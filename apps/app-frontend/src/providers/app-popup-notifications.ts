import { AbstractPopupNotificationManager, type PopupNotification } from '@modrinth/ui'
import { type Ref, ref } from 'vue'

export class AppPopupNotificationManager extends AbstractPopupNotificationManager {
	private static readonly STORAGE_KEY = 'axolotl:dismissed-popup-notifications'
	private readonly state: Ref<PopupNotification[]>
	private readonly dismissed = this.loadDismissed()

	public constructor() {
		super()
		this.state = ref<PopupNotification[]>([])
	}

	public getNotifications(): PopupNotification[] {
		return this.state.value
	}

	protected addNotificationToStorage(notification: PopupNotification): void {
		if (this.isDismissed(notification)) return
		this.state.value.unshift(notification)
	}

	protected removeNotificationFromStorage(id: string | number): void {
		const index = this.state.value.findIndex((n) => n.id === id)
		if (index > -1) {
			this.state.value.splice(index, 1)
		}
	}

	protected clearAllNotificationsFromStorage(): void {
		const keys = this.state.value.map((notification) => this.key(notification))
		this.state.value.splice(0)
		this.dismissed.clearedAt = Date.now()
		this.dismissed.keys = [...new Set([...this.dismissed.keys, ...keys])]
		this.saveDismissed()
	}

	private loadDismissed(): { clearedAt: number; keys: string[] } {
		try {
			const value = JSON.parse(
				localStorage.getItem(AppPopupNotificationManager.STORAGE_KEY) ?? '{}',
			)
			return {
				clearedAt: typeof value.clearedAt === 'number' ? value.clearedAt : 0,
				keys: Array.isArray(value.keys)
					? value.keys.filter((key: unknown) => typeof key === 'string')
					: [],
			}
		} catch {
			return { clearedAt: 0, keys: [] }
		}
	}

	private saveDismissed(): void {
		try {
			localStorage.setItem(AppPopupNotificationManager.STORAGE_KEY, JSON.stringify(this.dismissed))
		} catch {
			// Notification history is still usable when storage is unavailable.
		}
	}

	private key(notification: PopupNotification): string {
		return JSON.stringify([notification.title, notification.text ?? '', notification.type ?? ''])
	}

	private isDismissed(notification: PopupNotification): boolean {
		return (
			(notification.createdAt ?? Date.now()) <= this.dismissed.clearedAt ||
			this.dismissed.keys.includes(this.key(notification))
		)
	}

	public override removeNotification(id: string | number): void {
		const notification = this.state.value.find((item) => item.id === id)
		super.removeNotification(id)
		if (notification) {
			this.dismissed.keys = [...new Set([...this.dismissed.keys, this.key(notification)])]
			this.saveDismissed()
		}
	}
}
