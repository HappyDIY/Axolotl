<script setup lang="ts">
import { defineMessages, Toggle, useVIntl } from '@modrinth/ui'

import { getQuickScrollEnabled, setQuickScrollEnabled } from '@/helpers/scroll-top-state'
import { useTheming } from '@/store/theme'

import SettingsRow from './SettingsRow.vue'
import SettingsSection from './SettingsSection.vue'

const { formatMessage } = useVIntl()
const themeStore = useTheming()

themeStore.quickScrollEnabled = getQuickScrollEnabled()

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

function toggleQuickScroll(value: unknown) {
	themeStore.quickScrollEnabled = !!value
	setQuickScrollEnabled(themeStore.quickScrollEnabled)
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