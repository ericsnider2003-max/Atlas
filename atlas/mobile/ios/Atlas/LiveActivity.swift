import ActivityKit
import Foundation
import WidgetKit

/// The phone's live-activity card and Dynamic Island pill: what Atlas is
/// doing, and what's ready. Driven from the app (no push server, nothing
/// online): it reads /hub/live.json from Atlas on this phone.
@MainActor
final class LiveActivity {
    static let shared = LiveActivity()
    private var activity: Activity<AtlasActivity>?
    private var timer: Timer?

    func begin() {
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
        guard ActivityAuthorizationInfo().areActivitiesEnabled, let s = await AtlasCore.shared.live() else { return }
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
}
