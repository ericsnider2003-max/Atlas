import EventKit
import Foundation

/// The phone's calendar and Atlas's, kept together (H7). Reads the phone's own
/// calendars for a window (a week back, five weeks on), sends them to Atlas on
/// this phone (/hub/calendar/phone, calendar.rs `sync_from_phone`), and writes
/// Atlas's own events into one calendar of their own, "Atlas", which is never
/// read back, so nothing comes round twice. Only when you've allowed calendar
/// access; asked once, the first time it runs.
@MainActor
final class CalendarSync {
    static let shared = CalendarSync()
    private let store = EKEventStore()
    private var last = Date.distantPast
    /// Atlas's event id -> the phone's event identifier, so a changed event
    /// is updated rather than added again.
    private var written: [String: String] {
        get { UserDefaults.standard.dictionary(forKey: "atlas.calendar.written") as? [String: String] ?? [:] }
        set { UserDefaults.standard.set(newValue, forKey: "atlas.calendar.written") }
    }

    /// At most every ten minutes, and whenever the app comes forward.
    /// `ask`: you tapped "Bring in your calendar", so the system may ask
    /// for access now. Otherwise this only syncs once you've already said
    /// yes -- the app never asks at launch (the TestFlight review audit,
    /// 2 Oct 2026).
    func maybeSync(force: Bool = false, ask: Bool = false) {
        guard force || Date().timeIntervalSince(last) > 600 else { return }
        last = Date()
        Task { await sync(ask: ask) }
    }

    private func allowed(ask: Bool) async -> Bool {
        switch EKEventStore.authorizationStatus(for: .event) {
        case .fullAccess: return true
        case .notDetermined where ask: return (try? await store.requestFullAccessToEvents()) ?? false
        default: return false
        }
    }

    /// The phone's own calendar for Atlas's events, made the first time.
    private func atlasCalendar() -> EKCalendar? {
        if let c = store.calendars(for: .event).first(where: { $0.title == "Atlas" && $0.allowsContentModifications }) { return c }
        let c = EKCalendar(for: .event, eventStore: store)
        c.title = "Atlas"
        c.source = store.defaultCalendarForNewEvents?.source ?? store.sources.first(where: { $0.sourceType == .local })
        guard c.source != nil, (try? store.saveCalendar(c, commit: true)) != nil else { return nil }
        return c
    }

    private func sync(ask: Bool = false) async {
        guard await allowed(ask: ask), let url = AtlasCore.shared.url("/hub/calendar/phone"), let token = AtlasCore.shared.token else { return }
        let from = Calendar.current.date(byAdding: .day, value: -7, to: Date())!
        let to = Calendar.current.date(byAdding: .day, value: 35, to: Date())!
        let mine = atlasCalendar()
        let calendars = store.calendars(for: .event).filter { $0.calendarIdentifier != mine?.calendarIdentifier }
        let events = store.events(matching: store.predicateForEvents(withStart: from, end: to, calendars: calendars))
        let batch: [String: Any] = [
            "from": UInt64(from.timeIntervalSince1970),
            "to": UInt64(to.timeIntervalSince1970),
            "events": events.map { e -> [String: Any] in
                // A repeat's occurrences share an identifier; the start tells them apart.
                ["key": "\(e.calendarItemIdentifier)@\(UInt64(e.startDate.timeIntervalSince1970))",
                 "title": e.title ?? "",
                 "start": UInt64(e.startDate.timeIntervalSince1970),
                 "end": UInt64(e.endDate.timeIntervalSince1970),
                 "all_day": e.isAllDay,
                 "place": e.location ?? ""]
            },
        ]
        var r = URLRequest(url: url)
        r.httpMethod = "POST"
        r.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        r.httpBody = try? JSONSerialization.data(withJSONObject: batch)
        guard let (data, _) = try? await URLSession.shared.data(for: r),
              let reply = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let atlas = reply["atlas"] as? [[String: Any]], let cal = mine else { return }
        var written = self.written
        var seen = Set<String>()
        for a in atlas {
            guard let id = a["id"] as? String, let start = a["start"] as? Double, let end = a["end"] as? Double else { continue }
            seen.insert(id)
            let e = written[id].flatMap { store.event(withIdentifier: $0) } ?? EKEvent(eventStore: store)
            e.calendar = cal
            e.title = a["title"] as? String ?? "Atlas"
            e.startDate = Date(timeIntervalSince1970: start)
            e.endDate = Date(timeIntervalSince1970: end)
            e.isAllDay = a["all_day"] as? Bool ?? false
            e.location = a["place"] as? String
            if (try? store.save(e, span: .thisEvent, commit: false)) != nil { written[id] = e.eventIdentifier }
        }
        // Gone from Atlas (inside this window): gone from the phone.
        for (id, ident) in written where !seen.contains(id) {
            if let e = store.event(withIdentifier: ident), e.startDate >= from, e.startDate < to {
                try? store.remove(e, span: .thisEvent, commit: false)
                written[id] = nil
            } else if store.event(withIdentifier: ident) == nil {
                written[id] = nil
            }
        }
        try? store.commit()
        self.written = written
    }
}
