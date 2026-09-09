import type { RouteLocationNormalizedLoaded } from 'vue-router'

import {
	getLastBrowseContentProjectType,
	isBrowseContentProjectType,
} from '@/helpers/settings'

export type NavShortcutStoreKey =
	| 'shortcutNavHome'
	| 'shortcutNavWorlds'
	| 'shortcutNavDiscover'
	| 'shortcutNavSkins'
	| 'shortcutNavMultiplayer'
	| 'shortcutNavLibrary'
	| 'shortcutNavLab'
	| 'shortcutNavDownloads'
	| 'shortcutNavCreate'
	| 'shortcutNavSettings'

export interface NavShortcut {
	/** themeStore 开关字段名（默认关闭，设置页逐个启用）。 */
	id: NavShortcutStoreKey
	/** 与 Ctrl/Cmd 组合的键：数字 1-9 与 ','（Settings）。 */
	key: string
	labelKey: string
	descriptionKey: string
	labelDefault: string
	descriptionDefault: string
	target: (route: RouteLocationNormalizedLoaded) => string
}

/**
 * Resolves the Discover content target from the current route, mirroring the
 * logic previously owned by App.vue so the nav button and the keyboard
 * shortcut share one source of truth.
 */
export function discoverContentTarget(
	route: RouteLocationNormalizedLoaded,
): string {
	const projectType = route.params.projectType
	if (
		!route.query.i &&
		!route.query.sid &&
		!route.query.wid &&
		typeof projectType === 'string' &&
		isBrowseContentProjectType(projectType)
	) {
		return `/browse/${projectType}`
	}

	return `/browse/${getLastBrowseContentProjectType()}`
}

export const NAV_SHORTCUTS: NavShortcut[] = [
	{
		id: 'shortcutNavHome',
		key: '1',
		labelKey: 'app.shortcut-settings.nav-home',
		descriptionKey: 'app.shortcut-settings.nav-home-description',
		labelDefault: 'Home',
		descriptionDefault: 'Ctrl/Cmd + 1',
		target: () => '/',
	},
	{
		id: 'shortcutNavWorlds',
		key: '2',
		labelKey: 'app.shortcut-settings.nav-worlds',
		descriptionKey: 'app.shortcut-settings.nav-worlds-description',
		labelDefault: 'Worlds',
		descriptionDefault: 'Ctrl/Cmd + 2',
		target: () => '/worlds',
	},
	{
		id: 'shortcutNavDiscover',
		key: '3',
		labelKey: 'app.shortcut-settings.nav-discover',
		descriptionKey: 'app.shortcut-settings.nav-discover-description',
		labelDefault: 'Discover content',
		descriptionDefault: 'Ctrl/Cmd + 3',
		target: (route) => discoverContentTarget(route),
	},
	{
		id: 'shortcutNavSkins',
		key: '4',
		labelKey: 'app.shortcut-settings.nav-skins',
		descriptionKey: 'app.shortcut-settings.nav-skins-description',
		labelDefault: 'Skin selector',
		descriptionDefault: 'Ctrl/Cmd + 4',
		target: () => '/skins',
	},
	{
		id: 'shortcutNavMultiplayer',
		key: '5',
		labelKey: 'app.shortcut-settings.nav-multiplayer',
		descriptionKey: 'app.shortcut-settings.nav-multiplayer-description',
		labelDefault: 'Multiplayer',
		descriptionDefault: 'Ctrl/Cmd + 5',
		target: () => '/multiplayer',
	},
	{
		id: 'shortcutNavLibrary',
		key: '6',
		labelKey: 'app.shortcut-settings.nav-library',
		descriptionKey: 'app.shortcut-settings.nav-library-description',
		labelDefault: 'Library',
		descriptionDefault: 'Ctrl/Cmd + 6',
		target: () => '/library',
	},
	{
		id: 'shortcutNavLab',
		key: '7',
		labelKey: 'app.shortcut-settings.nav-lab',
		descriptionKey: 'app.shortcut-settings.nav-lab-description',
		labelDefault: 'Lab',
		descriptionDefault: 'Ctrl/Cmd + 7',
		target: () => '/lab',
	},
	{
		id: 'shortcutNavDownloads',
		key: '8',
		labelKey: 'app.shortcut-settings.nav-downloads',
		descriptionKey: 'app.shortcut-settings.nav-downloads-description',
		labelDefault: 'Downloads',
		descriptionDefault: 'Ctrl/Cmd + 8',
		target: () => '/downloads',
	},
	{
		id: 'shortcutNavCreate',
		key: '9',
		labelKey: 'app.shortcut-settings.nav-create',
		descriptionKey: 'app.shortcut-settings.nav-create-description',
		labelDefault: 'Create new instance',
		descriptionDefault: 'Ctrl/Cmd + 9',
		target: () => '/create',
	},
	{
		id: 'shortcutNavSettings',
		key: ',',
		labelKey: 'app.shortcut-settings.nav-settings',
		descriptionKey: 'app.shortcut-settings.nav-settings-description',
		labelDefault: 'Settings',
		descriptionDefault: 'Ctrl/Cmd + ,',
		target: () => '/settings',
	},
]