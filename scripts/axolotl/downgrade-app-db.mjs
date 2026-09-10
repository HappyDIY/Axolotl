// Downgrade the launcher app database so an older build can open it again.
//
// sqlx refuses to open a database whose applied migrations include a version the
// running binary does not know ("... was previously applied but is missing in
// the resolved migrations"), which leaves the launcher unable to start. This is
// what happens after a build that carried a new migration wrote it into a
// database that an older build - or an installed release - then tries to open.
//
// The upgrade path is one-way, so undoing it means removing the newer migration
// records and the schema they added. Both halves are needed: dropping only the
// records would make the next newer build fail again, this time because the
// column it adds already exists.
//
// Usage (dry run unless --apply is given):
//   node scripts/axolotl/downgrade-app-db.mjs --list
//   node scripts/axolotl/downgrade-app-db.mjs --to <version>
//   node scripts/axolotl/downgrade-app-db.mjs --to <version> --apply
//
//   --to <version>   remove every applied migration at or after this version
//   --db <path>      target app.db (default: the active channel directory)
//   --channel <name> pick a channel directory instead of reading the active one
//   --list           print the applied migrations and exit
//
// The launcher must be closed: a running instance keeps the database open and
// would keep using the schema it read at startup.

import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'

const SETTINGS_DIR_NAME = 'red.ghs.axolotl'
const APP_DB = 'app.db'
const SIDECARS = ['-wal', '-shm']

// Schema added by known migrations, used to undo it. An entry is required for
// every migration that adds a column: `ALTER TABLE ... ADD COLUMN` is not
// reversible in place, and leaving the column behind breaks the next install of
// a build that carries the migration. Migrations that only move data (DELETE,
// UPDATE) need no entry - replaying them is harmless.
const REVERTIBLE_COLUMNS = {
	// settings.close_behavior
	20260903120000: [{ table: 'settings', column: 'close_behavior' }],
	// settings.log_level; recorded ahead of the build that ships it
	20260909010000: [{ table: 'settings', column: 'log_level' }],
}

function fail(message) {
	console.error(`error: ${message}`)
	process.exit(1)
}

function settingsBaseDir() {
	const override = process.env.THESEUS_CONFIG_DIR
	if (override) return override

	const appData = process.env.APPDATA
	if (!appData) fail('APPDATA is not set; pass --db explicitly')
	return join(appData, SETTINGS_DIR_NAME)
}

function resolveDatabase(args) {
	if (args.db) return args.db

	const base = settingsBaseDir()
	let channel = args.channel
	if (!channel) {
		const statePath = join(base, 'update-channel.json')
		if (existsSync(statePath)) {
			try {
				channel = JSON.parse(readFileSync(statePath, 'utf8')).active_channel ?? undefined
			} catch {
				console.warn(`warning: could not parse ${statePath}; assuming beta`)
			}
		}
		channel ??= 'beta'
	}

	return join(base, channel, APP_DB)
}

function parseArgs(argv) {
	const args = {}
	for (let index = 0; index < argv.length; index += 1) {
		const token = argv[index]
		switch (token) {
			case '--to': {
				const value = argv[++index]
				const parsed = Number(value)
				if (!Number.isInteger(parsed) || parsed <= 0) fail(`invalid --to value: ${value}`)
				args.to = parsed
				break
			}
			case '--db':
				args.db = argv[++index]
				break
			case '--channel':
				args.channel = argv[++index]
				break
			case '--apply':
				args.apply = true
				break
			case '--list':
				args.list = true
				break
			default:
				fail(`unknown argument: ${token}`)
		}
	}

	return args
}

function printUsage() {
	console.log(
		[
			'Downgrade the launcher app database so an older build can open it.',
			'',
			'  node scripts/axolotl/downgrade-app-db.mjs --list',
			'  node scripts/axolotl/downgrade-app-db.mjs --to <version> [--apply] [--db <path>] [--channel <name>]',
			'',
			'Without --apply the script only reports what it would change.',
			'Undo an accidental change with the timestamped backup it writes.',
		].join('\n'),
	)
}

function listMigrations(db) {
	const database = new DatabaseSync(db, { readOnly: true })
	const rows = database
		.prepare('SELECT version, description, success FROM _sqlx_migrations ORDER BY version')
		.all()
	database.close()

	console.log(`database: ${db}`)
	console.log(`${rows.length} applied migrations`)
	for (const row of rows) {
		console.log(`  ${row.version}  ${row.description}  success=${row.success}`)
	}
}

function launcherIsRunning() {
	const result = spawnSync('tasklist', ['/FI', 'IMAGENAME eq Axolotl Launcher.exe', '/NH'], {
		encoding: 'utf8',
	})
	return `${result.stdout ?? ''}${result.stderr ?? ''}`.includes('Axolotl Launcher.exe')
}

function columnsPresent(db, columns) {
	if (columns.length === 0) return []

	const database = new DatabaseSync(db, { readOnly: true })
	const present = columns.map(({ table, column }) => {
		const names = database
			.prepare('SELECT name FROM pragma_table_info(?)')
			.all(table)
			.map((row) => row.name)
		return { table, column, present: names.includes(column) }
	})
	database.close()
	return present
}

function main() {
	const args = parseArgs(process.argv.slice(2))
	if (!args.list && !args.to) {
		printUsage()
		return
	}

	const db = resolveDatabase(args)
	if (!existsSync(db)) fail(`no database at ${db}`)

	if (args.list) {
		listMigrations(db)
		return
	}

	const database = new DatabaseSync(db, { readOnly: true })
	const applied = database
		.prepare(
			'SELECT version FROM _sqlx_migrations WHERE success = TRUE AND version >= ? ORDER BY version',
		)
		.all(args.to)
		.map((row) => Number(row.version))
	database.close()

	if (applied.length === 0) {
		console.log(`nothing to do: ${db} has no applied migration at or after ${args.to}`)
		return
	}

	const plan = []
	for (const version of applied) {
		const mapped = REVERTIBLE_COLUMNS[version]
		plan.push({
			version,
			columns: columnsPresent(db, mapped ?? []),
			mapped: mapped !== undefined,
		})
	}

	console.log(`database: ${db}`)
	console.log(`migrations to remove: ${applied.join(', ')}`)
	for (const { version, columns, mapped } of plan) {
		if (!mapped) {
			console.log(
				`  ${version}: no known schema for this migration, removing its record only`,
			)
			continue
		}
		for (const { table, column, present } of columns) {
			console.log(
				present
					? `  ${version}: drop ${table}.${column}`
					: `  ${version}: ${table}.${column} already absent`,
			)
		}
	}

	const unmapped = plan.filter(({ mapped }) => !mapped).map(({ version }) => version)
	if (unmapped.length > 0) {
		console.log(
			`\nwarning: ${unmapped.join(', ')} have no known schema in this script, so any\n` +
				'columns or tables they added are left in place. Reinstalling a build that\n' +
				'carries them will then fail on the duplicate object.',
		)
	}

	if (!args.apply) {
		console.log('\ndry run: rerun with --apply to perform the downgrade')
		return
	}

	if (launcherIsRunning()) {
		fail('Axolotl Launcher is running; close it before downgrading the database')
	}

	const stamp = new Date().toISOString().replace(/[:.]/g, '-')
	const backup = `${db}.before-downgrade-${stamp}`
	copyFileSync(db, backup)
	for (const suffix of SIDECARS) {
		if (existsSync(`${db}${suffix}`)) copyFileSync(`${db}${suffix}`, `${backup}${suffix}`)
	}
	console.log(`\nbackup: ${backup}`)

	const writable = new DatabaseSync(db)
	writable.exec('PRAGMA foreign_keys = OFF')
	writable.exec('BEGIN')
	try {
		for (const { columns } of plan) {
			for (const { table, column, present } of columns) {
				if (present) writable.exec(`ALTER TABLE ${table} DROP COLUMN ${column}`)
			}
		}
		writable
			.prepare(`DELETE FROM _sqlx_migrations WHERE version IN (${applied.join(', ')})`)
			.run()
		writable.exec('COMMIT')
	} catch (error) {
		writable.exec('ROLLBACK')
		writable.close()
		fail(`downgrade failed, database left untouched (backup: ${backup}): ${error.message}`)
	}

	const remaining = writable.prepare('SELECT COUNT(*) AS count FROM _sqlx_migrations').get().count
	const integrity = writable.prepare('PRAGMA integrity_check').get().integrity_check
	writable.close()

	console.log(`migration records remaining: ${remaining}`)
	console.log(`integrity_check: ${integrity}`)
	console.log(
		'\nDone. An older build can open this database again. Reinstalling a build that\n' +
			'carries the removed migrations applies them anew and restores what was dropped.',
	)
}

main()
