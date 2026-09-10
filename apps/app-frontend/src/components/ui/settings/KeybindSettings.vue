<script setup lang="ts">
import { defineMessages, Toggle, useVIntl } from '@modrinth/ui'

import { getNavShortcutEnabled, setNavShortcutEnabled } from '@/helpers/nav-shortcut-state'
import { NAV_SHORTCUTS, type NavShortcut } from '@/helpers/nav-shortcuts'
import { getQuickScrollEnabled, setQuickScrollEnabled } from '@/helpers/scroll-top-state'
import { useTheming } from '@/store/theme'

import SettingsRow from './SettingsRow.vue'
import SettingsSection from './SettingsSection.vue'

const { formatMessage } = useVIntl()
const themeStore = useTheming()

themeStore.quickScrollEnabled = getQuickScrollEnabled()
for (const shortcut of NAV_SHORTCUTS) {
	themeStore[shortcut.id] = getNavShortcutEnabled(shortcut.id)
}

const messages = defineMessages({
	scrollTitle: {
		id: 'app.shortcut-settings.scroll-title',
		defaultMessage: 'Quick scrolling',
	},
	scrollDescription: {
		id: 'app.shortcut-settings.scroll-description',
		defaultMessage: 'Move through long pages with the keyboard.',
	},
	enable: { id: 'app.shortcut-settings.enable', defaultMessage: 'Enable quick scrolling' },
	homeAction: {
		id: 'app.shortcut-settings.home-action',
		defaultMessage: 'Scroll to the top',
	},
	endAction: {
		id: 'app.shortcut-settings.end-action',
		defaultMessage: 'Scroll to the bottom',
	},
	pageUpAction: {
		id: 'app.shortcut-settings.page-up-action',
		defaultMessage: 'Scroll up one screen',
	},
	pageDownAction: {
		id: 'app.shortcut-settings.page-down-action',
		defaultMessage: 'Scroll down one screen',
	},
	navTitle: {
		id: 'app.shortcut-settings.nav-title',
		defaultMessage: 'Navigation shortcuts',
	},
	navDescription: {
		id: 'app.shortcut-settings.nav-description',
		defaultMessage:
			'Jump to a menu item with Ctrl/Cmd + a number. Each shortcut is off until enabled.',
	},
})

function shortcutLabel(shortcut: NavShortcut) {
	return formatMessage({ id: shortcut.labelKey, defaultMessage: shortcut.labelDefault })
}

function shortcutDescription(shortcut: NavShortcut) {
	return formatMessage({
		id: shortcut.descriptionKey,
		defaultMessage: shortcut.descriptionDefault,
	})
}

function toggleQuickScroll(value: unknown) {
	themeStore.quickScrollEnabled = !!value
	setQuickScrollEnabled(themeStore.quickScrollEnabled)
}

function toggleNavShortcut(shortcut: NavShortcut, value: unknown) {
	themeStore[shortcut.id] = !!value
	setNavShortcutEnabled(shortcut.id, themeStore[shortcut.id])
}
</script>

<template>
	<div class="flex flex-col gap-6">
		<SettingsSection>
			<template #header>
				<h2 class="m-0 text-lg font-semibold text-contrast">
					{{ formatMessage(messages.scrollTitle) }}
				</h2>
				<p class="m-0 mt-1 text-sm leading-relaxed text-secondary">
					{{ formatMessage(messages.scrollDescription) }}
				</p>
			</template>
			<SettingsRow>
				<template #label>
					<span id="settings-target-shortcuts-enable" tabindex="-1">
						{{ formatMessage(messages.enable) }}
					</span>
				</template>
				<template #control>
					<Toggle
						id="quick-scroll-enabled"
						:model-value="themeStore.quickScrollEnabled"
						@update:model-value="toggleQuickScroll"
					/>
				</template>
			</SettingsRow>
			<SettingsRow>
				<template #label>
					<span id="settings-target-shortcuts-home" tabindex="-1">
						{{ formatMessage(messages.homeAction) }}
					</span>
				</template>
				<template #control>
					<kbd
						class="rounded-md border border-solid border-surface-4 bg-surface-3 px-2 py-0.5 font-mono text-sm text-contrast"
					>
						Home
					</kbd>
				</template>
			</SettingsRow>
			<SettingsRow>
				<template #label>
					<span id="settings-target-shortcuts-end" tabindex="-1">
						{{ formatMessage(messages.endAction) }}
					</span>
				</template>
				<template #control>
					<kbd
						class="rounded-md border border-solid border-surface-4 bg-surface-3 px-2 py-0.5 font-mono text-sm text-contrast"
					>
						End
					</kbd>
				</template>
			</SettingsRow>
			<SettingsRow>
				<template #label>
					<span id="settings-target-shortcuts-page-up" tabindex="-1">
						{{ formatMessage(messages.pageUpAction) }}
					</span>
				</template>
				<template #control>
					<kbd
						class="rounded-md border border-solid border-surface-4 bg-surface-3 px-2 py-0.5 font-mono text-sm text-contrast"
					>
						Page Up
					</kbd>
				</template>
			</SettingsRow>
			<SettingsRow>
				<template #label>
					<span id="settings-target-shortcuts-page-down" tabindex="-1">
						{{ formatMessage(messages.pageDownAction) }}
					</span>
				</template>
				<template #control>
					<kbd
						class="rounded-md border border-solid border-surface-4 bg-surface-3 px-2 py-0.5 font-mono text-sm text-contrast"
					>
						Page Down
					</kbd>
				</template>
			</SettingsRow>
		</SettingsSection>

		<SettingsSection>
			<template #header>
				<h2
					id="settings-target-shortcuts-nav"
					tabindex="-1"
					class="m-0 text-lg font-semibold text-contrast"
				>
					{{ formatMessage(messages.navTitle) }}
				</h2>
				<p class="m-0 mt-1 text-sm leading-relaxed text-secondary">
					{{ formatMessage(messages.navDescription) }}
				</p>
			</template>
			<SettingsRow v-for="shortcut in NAV_SHORTCUTS" :key="shortcut.id">
				<template #label>{{ shortcutLabel(shortcut) }}</template>
				<template #description>{{ shortcutDescription(shortcut) }}</template>
				<template #control>
					<Toggle
						:id="`nav-shortcut-${shortcut.id}`"
						:model-value="themeStore[shortcut.id]"
						@update:model-value="(value) => toggleNavShortcut(shortcut, value)"
					/>
				</template>
			</SettingsRow>
		</SettingsSection>
	</div>
</template>
