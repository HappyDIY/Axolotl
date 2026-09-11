import type { KeyBinding } from '@modrinth/ui'
import type { RouteLocationNormalizedLoaded } from 'vue-router'

import { getLastBrowseContentProjectType, isBrowseContentProjectType } from '@/helpers/settings'

/** Which half of the settings page an action belongs to. */
export type ShortcutGroupId = 'scroll' | 'nav'

/** Theme store fields that switch an action on. */
export type ShortcutEnabledField =
	| 'quickScrollEnabled'
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

export interface ShortcutContext {
	worldsTabEnabled: boolean
	offline: boolean
}

export interface ShortcutAction {
	/** Stable id, used for the binding, the enable flag and the settings row. */
	id: string
	group: ShortcutGroupId
	enabledField: ShortcutEnabledField
	defaultBinding: KeyBinding
	labelKey: string
	labelDefault: string
	descriptionKey: string
	descriptionDefault: string
	/** Navigation actions know where they lead. */
	target?: (route: RouteLocationNormalizedLoaded) => string
	/** Navigation actions that are not reachable right now. */
	unavailable?: (context: ShortcutContext) => boolean
	/** Scrolling actions move the page's own scroller. */
	applyScroll?: (scroller: HTMLElement) => void
}

function keyboard(code: string, mod = false, alt = false, shift = false): KeyBinding {
	return { device: 'keyboard', code, mod, alt, shift }
}

/**
 * Resolves the Discover content target from the current route, so the nav
 * button and the keyboard shortcut share one source of truth.
 */
export function discoverContentTarget(route: RouteLocationNormalizedLoaded): string {
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

/**
 * Every shortcut the launcher answers to. The binding here is the one a fresh
 * install uses; a recorded binding is stored separately and wins over it.
 */
export const SHORTCUT_ACTIONS: ShortcutAction[] = [
	{
		id: 'shortcutScrollTop',
		group: 'scroll',
		enabledField: 'quickScrollEnabled',
		defaultBinding: keyboard('Home'),
		labelKey: 'app.shortcut-settings.home-action',
		labelDefault: 'Scroll to the top',
		descriptionKey: 'app.shortcut-settings.home-action-description',
		descriptionDefault: 'Moves the page you are reading to the top.',
		applyScroll: (scroller) => scroller.scrollTo({ top: 0, behavior: 'smooth' }),
	},
	{
		id: 'shortcutScrollBottom',
		group: 'scroll',
		enabledField: 'quickScrollEnabled',
		defaultBinding: keyboard('End'),
		labelKey: 'app.shortcut-settings.end-action',
		labelDefault: 'Scroll to the bottom',
		descriptionKey: 'app.shortcut-settings.end-action-description',
		descriptionDefault: 'Moves the page you are reading to the bottom.',
		applyScroll: (scroller) =>
			scroller.scrollTo({ top: scroller.scrollHeight, behavior: 'smooth' }),
	},
	{
		id: 'shortcutScrollUp',
		group: 'scroll',
		enabledField: 'quickScrollEnabled',
		defaultBinding: keyboard('PageUp'),
		labelKey: 'app.shortcut-settings.page-up-action',
		labelDefault: 'Scroll up one screen',
		descriptionKey: 'app.shortcut-settings.page-up-action-description',
		descriptionDefault: 'Moves the page you are reading up by one screen.',
		applyScroll: (scroller) => {
			// Immediate scrolling keeps rapid key repeats responsive.
			scroller.scrollTop = Math.max(0, scroller.scrollTop - scroller.clientHeight * 0.9)
		},
	},
	{
		id: 'shortcutScrollDown',
		group: 'scroll',
		enabledField: 'quickScrollEnabled',
		defaultBinding: keyboard('PageDown'),
		labelKey: 'app.shortcut-settings.page-down-action',
		labelDefault: 'Scroll down one screen',
		descriptionKey: 'app.shortcut-settings.page-down-action-description',
		descriptionDefault: 'Moves the page you are reading down by one screen.',
		applyScroll: (scroller) => {
			scroller.scrollTop = Math.min(
				scroller.scrollHeight,
				scroller.scrollTop + scroller.clientHeight * 0.9,
			)
		},
	},
	{
		id: 'shortcutNavHome',
		group: 'nav',
		enabledField: 'shortcutNavHome',
		defaultBinding: keyboard('Digit1', true),
		labelKey: 'app.shortcut-settings.nav-home',
		labelDefault: 'Home',
		descriptionKey: 'app.shortcut-settings.nav-home-description',
		descriptionDefault: 'Jump to the Home page.',
		target: () => '/',
	},
	{
		id: 'shortcutNavWorlds',
		group: 'nav',
		enabledField: 'shortcutNavWorlds',
		defaultBinding: keyboard('Digit2', true),
		labelKey: 'app.shortcut-settings.nav-worlds',
		labelDefault: 'Worlds',
		descriptionKey: 'app.shortcut-settings.nav-worlds-description',
		descriptionDefault: 'Jump to your worlds.',
		target: () => '/worlds',
		unavailable: (context) => !context.worldsTabEnabled,
	},
	{
		id: 'shortcutNavDiscover',
		group: 'nav',
		enabledField: 'shortcutNavDiscover',
		defaultBinding: keyboard('Digit3', true),
		labelKey: 'app.shortcut-settings.nav-discover',
		labelDefault: 'Discover content',
		descriptionKey: 'app.shortcut-settings.nav-discover-description',
		descriptionDefault: 'Jump to browsing content.',
		target: (route) => discoverContentTarget(route),
		unavailable: (context) => context.offline,
	},
	{
		id: 'shortcutNavSkins',
		group: 'nav',
		enabledField: 'shortcutNavSkins',
		defaultBinding: keyboard('Digit4', true),
		labelKey: 'app.shortcut-settings.nav-skins',
		labelDefault: 'Skin selector',
		descriptionKey: 'app.shortcut-settings.nav-skins-description',
		descriptionDefault: 'Jump to the skin selector.',
		target: () => '/skins',
	},
	{
		id: 'shortcutNavMultiplayer',
		group: 'nav',
		enabledField: 'shortcutNavMultiplayer',
		defaultBinding: keyboard('Digit5', true),
		labelKey: 'app.shortcut-settings.nav-multiplayer',
		labelDefault: 'Multiplayer',
		descriptionKey: 'app.shortcut-settings.nav-multiplayer-description',
		descriptionDefault: 'Jump to multiplayer.',
		target: () => '/multiplayer',
	},
	{
		id: 'shortcutNavLibrary',
		group: 'nav',
		enabledField: 'shortcutNavLibrary',
		defaultBinding: keyboard('Digit6', true),
		labelKey: 'app.shortcut-settings.nav-library',
		labelDefault: 'Library',
		descriptionKey: 'app.shortcut-settings.nav-library-description',
		descriptionDefault: 'Jump to your library.',
		target: () => '/library',
	},
	{
		id: 'shortcutNavLab',
		group: 'nav',
		enabledField: 'shortcutNavLab',
		defaultBinding: keyboard('Digit7', true),
		labelKey: 'app.shortcut-settings.nav-lab',
		labelDefault: 'Lab',
		descriptionKey: 'app.shortcut-settings.nav-lab-description',
		descriptionDefault: 'Jump to the Lab.',
		target: () => '/lab',
	},
	{
		id: 'shortcutNavDownloads',
		group: 'nav',
		enabledField: 'shortcutNavDownloads',
		defaultBinding: keyboard('Digit8', true),
		labelKey: 'app.shortcut-settings.nav-downloads',
		labelDefault: 'Downloads',
		descriptionKey: 'app.shortcut-settings.nav-downloads-description',
		descriptionDefault: 'Jump to downloads.',
		target: () => '/downloads',
	},
	{
		id: 'shortcutNavCreate',
		group: 'nav',
		enabledField: 'shortcutNavCreate',
		defaultBinding: keyboard('Digit9', true),
		labelKey: 'app.shortcut-settings.nav-create',
		labelDefault: 'Create new instance',
		descriptionKey: 'app.shortcut-settings.nav-create-description',
		descriptionDefault: 'Jump to creating a new instance.',
		target: () => '/create',
		unavailable: (context) => context.offline,
	},
	{
		id: 'shortcutNavSettings',
		group: 'nav',
		enabledField: 'shortcutNavSettings',
		defaultBinding: keyboard('Comma', true),
		labelKey: 'app.shortcut-settings.nav-settings',
		labelDefault: 'Settings',
		descriptionKey: 'app.shortcut-settings.nav-settings-description',
		descriptionDefault: 'Jump to the settings.',
		target: () => '/settings',
	},
]

export const SCROLL_ACTIONS = SHORTCUT_ACTIONS.filter((action) => action.group === 'scroll')
export const NAV_ACTIONS = SHORTCUT_ACTIONS.filter((action) => action.group === 'nav')

export function shortcutAction(id: string): ShortcutAction | undefined {
	return SHORTCUT_ACTIONS.find((action) => action.id === id)
}
