import ActivityKit
import Foundation
import UserNotifications
import WidgetKit

/// The phone's live-activity card and Dynamic Island pill: what Atlas is
/// doing, and what's ready. Driven from the app (no push server, nothing
/// online): it reads /hub/live.json from Atlas on this phone.
@MainActor
final class LiveActivity {
    static let shared = LiveActivity()
    private var activity: Activity<AtlasActivity>?
    private var timer: Timer?
    /// The last of Atlas's own lines shown (live.json "said"): each reminder
    /// or finished job becomes one notification (30 Sep 2026: the phone's
    /// background lines were dropped, so reminders never appeared).
    private var lastSaid: Int64 = 0
    private var askedToNotify = false

    /// The reminders handed to iOS when the app last went to the background.
    private var ahead: [LiveState.Upcoming] = []

    func begin() {
        // In front again: Atlas rings its own reminders, so the ones handed
        // to iOS are taken back -- never both (item 15).
        Reminders.takeBack()
        // Runs whether or not Live Activities are allowed: the widgets' glance
        // is refreshed by the same loop.
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { _ in Task { await self.update() } }
        Task { await update() }
    }

    func refresh() { Task { await update() } }

    /// Going into the background: the app stops reading Atlas, so the card
    /// would go on showing what was true when it left. End it (28 Sep 2026:
    /// it stayed, stale, with no stale date), after one last glance update.
    func background() {
        timer?.invalidate()
        timer = nil
        Task {
            if let g = await AtlasCore.shared.glance(), GlanceStore.keep(g) {
                WidgetCenter.shared.reloadAllTimelines()
            }
            // Reminders still to come, handed to iOS: they ring on time with
            // the app closed (item 15). The last list read stands in if Atlas
            // doesn't answer in time.
            let upcoming = await AtlasCore.shared.live()?.upcoming ?? self.ahead
            await Reminders.handOver(upcoming)
            await activity?.end(nil, dismissalPolicy: .immediate)
            activity = nil
        }
    }

    private func update() async {
        // The widgets ride the same loop: while the app runs, the glance it
        // leaves in the app group stays current, and WidgetKit is asked to
        // redraw only when what shows has changed.
        CalendarSync.shared.maybeSync()
        if let g = await AtlasCore.shared.glance(), GlanceStore.keep(g) {
            WidgetCenter.shared.reloadAllTimelines()
        }
        guard let s = await AtlasCore.shared.live() else { return }
        ahead = s.upcoming ?? []
        await tell(s.said ?? [])
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return }
        let state = AtlasActivity.ContentState(
            status: s.status,
            doing: s.working?.title ?? "",
            step: s.working?.step ?? "",
            ready: s.ready.first?.title ?? "",
            waiting: s.waiting)
        // Stale two minutes on: if the app stops updating it (suspended,
        // killed), the system shows it as out of date rather than current.
        let content = ActivityContent(state: state, staleDate: Date().addingTimeInterval(120))
        // Nothing working and nothing ready: no card. The pill is only there
        // when there's something to see — the locked interrupt rule.
        if s.working == nil && s.ready.isEmpty {
            await activity?.end(content, dismissalPolicy: .immediate)
            activity = nil
            return
        }
        if let a = activity {
            await a.update(content)
        } else {
            activity = try? Activity.request(attributes: AtlasActivity(), content: content)
        }
    }

    /// Each new line Atlas said, as a notification.
    private func tell(_ said: [LiveState.Said]) async {
        let fresh = said.filter { $0.id > lastSaid }
        guard !fresh.isEmpty else { return }
        lastSaid = fresh.map(\.id).max() ?? lastSaid
        let center = UNUserNotificationCenter.current()
        if !askedToNotify {
            askedToNotify = true
            _ = try? await center.requestAuthorization(options: [.alert, .sound])
        }
        for line in fresh {
            let c = UNMutableNotificationContent()
            c.title = "Atlas"
            c.body = line.text
            c.sound = .default
            try? await center.add(UNNotificationRequest(identifier: "atlas-said-\(line.id)", content: c, trigger: nil))
        }
    }
}

/// Reminders handed to iOS while Atlas is away (item 15). Each is a local
/// notification at its time -- the phone's own scheduler, nothing online.
enum Reminders {
    static let prefix = "atlas-reminder-"

    static func handOver(_ upcoming: [LiveState.Upcoming]) async {
        let center = UNUserNotificationCenter.current()
        _ = try? await center.requestAuthorization(options: [.alert, .sound])
        await takeBackNow()
        let now = Date().timeIntervalSince1970
        for r in upcoming where Double(r.due) > now + 1 {
            let c = UNMutableNotificationContent()
            c.title = "Atlas"
            c.body = r.text
            c.sound = .default
            let trigger = UNTimeIntervalNotificationTrigger(timeInterval: Double(r.due) - now, repeats: false)
            try? await center.add(UNNotificationRequest(identifier: "\(prefix)\(r.id)-\(r.due)", content: c, trigger: trigger))
        }
    }

    static func takeBack() { Task { await takeBackNow() } }

    private static func takeBackNow() async {
        let center = UNUserNotificationCenter.current()
        let ids = await center.pendingNotificationRequests().map(\.identifier).filter { $0.hasPrefix(prefix) }
        center.removePendingNotificationRequests(withIdentifiers: ids)
    }
}
