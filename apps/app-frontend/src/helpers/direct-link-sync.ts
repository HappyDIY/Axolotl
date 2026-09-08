import {
	type DirectLinkSyncReport,
	type ExternalMinecraftRoot,
	sync_direct_links,
} from './instance'

export const DIRECT_LINKS_SYNCED_EVENT = 'axolotl-direct-links-synced'

let requestedRoots: ExternalMinecraftRoot[] = []
let syncWorker: Promise<void> | undefined
let syncPending = false

/**
 * Serializes direct-link reconciliation across Settings, routing, and focus
 * events. Each request records a fresh snapshot; changes received during an
 * in-flight reconciliation always run immediately afterwards.
 */
export function syncConfiguredDirectLinks(roots: readonly ExternalMinecraftRoot[]): Promise<void> {
	requestedRoots = roots.map((root) => ({ ...root }))
	syncPending = true
	if (!syncWorker) {
		syncWorker = drainSyncRequests().finally(() => {
			syncWorker = undefined
		})
	}
	return syncWorker
}

async function drainSyncRequests() {
	while (syncPending) {
		syncPending = false
		const report = await sync_direct_links(requestedRoots)
		window.dispatchEvent(
			new CustomEvent<DirectLinkSyncReport>(DIRECT_LINKS_SYNCED_EVENT, { detail: report }),
		)
	}
}
