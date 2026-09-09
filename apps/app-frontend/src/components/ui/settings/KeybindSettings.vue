<script setup lang="ts">
import { defineMessages, SettingsLabel, Toggle, useVIntl } from '@modrinth/ui'

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
	title: { id: 'app.shortcut-settings.title', defaultMessage: 'Keyboard shortcuts' },
	description: {
		id: 'app.shortcut-settings.description',
		defaultMessage: 'Browse, home and library screens support quick scrolling keys.',
	},
	enable: { id: 'app.shortcut-settings.enable', defaultMessage: 'Enable quick scrolling' },
	enableDescription: {
		id: 'app.shortcut-settings.enable-description',
		defaultMessage: 'Allow Home / End / Page Up / Page Down to scroll the page.',
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
	homeDescription: {
		id: 'app.shortcut-settings.key-home-description',
		defaultMessage: 'Scroll to the top of the page.',
	},
	endDescription: {
		id: 'app.shortcut-settings.key-end-description',
		defaultMessage: 'Scroll to the bottom of the page.',
	},
	pageUpDescription: {
		id: 'app.shortcut-settings.key-page-up-description',
		defaultMessage: 'Scroll up by one viewport.',
	},
	pageDownDescription: {
		id: 'app.shortcut-settings.key-page-down-description',
		defaultMessage: 'Scroll down by one viewport.',
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
				<h2 id="settings-target-shortcuts" tabindex="-1" class="m-0 text-lg font-semibold text-contrast">
					{{ formatMessage(messages.title) }}
				</h2>
				<p class="m-0 mt-1 text-sm leading-relaxed text-secondary">
					{{ formatMessage(messages.description) }}
				</p>
			</template>
			<SettingsRow stacked>
				<template #label>
					<span id="settings-target-shortcuts-enable" tabindex="-1">
						{{ formatMessage(messages.enable) }}
					</span>
				</template>
				<template #description>{{ formatMessage(messages.enableDescription) }}</template>
				<template #control>
					<Toggle
						id="quick-scroll-enabled"
						:model-value="themeStore.quickScrollEnabled"
						@update:model-value="toggleQuickScroll"
					/>
				</template>
			</SettingsRow>

			<SettingsLabel
				:title="formatMessage(messages.navTitle)"
				:description="formatMessage(messages.navDescription)"
			/>
			<SettingsRow
				v-for="shortcut in NAV_SHORTCUTS"
				:key="shortcut.id"
				stacked
			>
				<template #label>
					<span :id="`settings-target-shortcuts-nav-${shortcut.id}`" tabindex="-1">
						{{ shortcutLabel(shortcut) }}
					</span>
				</template>
				<template #description>{{ shortcutDescription(shortcut) }}</template>
				<template #control>
					<Toggle
						:id="`nav-shortcut-${shortcut.id}`"
						:model-value="themeStore[shortcut.id]"
						@update:model-value="(value) => toggleNavShortcut(shortcut, value)"
					/>
				</template>
			</SettingsRow>

			<SettingsRow>
				<template #label>
					<kbd
						id="settings-target-shortcuts-home"
						tabindex="-1"
						class="rounded-md border border-solid border-surface-4 bg-surface-3 px-2 py-0.5 font-mono text-sm text-contrast"
					>
						Home
					</kbd>
				</template>
				<template #description>{{ formatMessage(messages.homeDescription) }}</template>
			</SettingsRow>
			<SettingsRow>
				<template #label>
					<kbd
						id="settings-target-shortcuts-end"
						tabindex="-1"
						class="rounded-md border border-solid border-surface-4 bg-surface-3 px-2 py-0.5 font-mono text-sm text-contrast"
					>
						End
					</kbd>
				</template>
				<template #description>{{ formatMessage(messages.endDescription) }}</template>
			</SettingsRow>
			<SettingsRow>
				<template #label>
					<kbd
						id="settings-target-shortcuts-page-up"
						tabindex="-1"
						class="rounded-md border border-solid border-surface-4 bg-surface-3 px-2 py-0.5 font-mono text-sm text-contrast"
					>
						Page Up
					</kbd>
				</template>
				<template #description>{{ formatMessage(messages.pageUpDescription) }}</template>
			</SettingsRow>
			<SettingsRow>
				<template #label>
					<kbd
						id="settings-target-shortcuts-page-down"
						tabindex="-1"
						class="rounded-md border border-solid border-surface-4 bg-surface-3 px-2 py-0.5 font-mono text-sm text-contrast"
					>
						Page Down
					</kbd>
				</template>
				<template #description>{{ formatMessage(messages.pageDownDescription) }}</template>
			</SettingsRow>
		</SettingsSection>
	</div>
</template>