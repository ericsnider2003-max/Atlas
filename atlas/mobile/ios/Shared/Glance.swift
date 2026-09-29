import Foundation

/// What the home-screen and lock-screen widgets show: the core's
/// /hub/glance.json (`glance.rs`), made safe on the laptop side before it
/// ever gets here. The app fetches it while it runs and leaves it in the app
/// group's folder; the widget, which can't run Atlas or reach it, reads it
/// from there. No token and nothing else crosses.
struct Glance: Codable, Equatable {
    struct Next: Codable, Equatable { let at: String; let what: String }
    struct View: Codable, Equatable { let working: String?; let next: Next?; let waiting: Int }
    let as_of: UInt64
    let status: String
    let tone: String
    let home: View
    let lock: View
    let capture: String

    /// Before the app has ever run, or if it couldn't write.
    static let unknown = Glance(as_of: 0, status: "Open Atlas once", tone: "off",
                                home: View(working: nil, next: nil, waiting: 0),
                                lock: View(working: nil, next: nil, waiting: 0),
                                capture: "atlas://hub/give")

    /// Minutes since the core made this, for "as of 14:05".
    func age(now: Date = Date()) -> TimeInterval { now.timeIntervalSince1970 - TimeInterval(as_of) }

    /// Written more than 15 minutes ago: the widget says when, rather than
    /// passing old news off as current.
    var isStale: Bool { as_of == 0 || age() > 15 * 60 }

    var asOf: String {
        let f = DateFormatter(); f.timeStyle = .short; f.dateStyle = .none
        return f.string(from: Date(timeIntervalSince1970: TimeInterval(as_of)))
    }
}

enum GlanceStore {
    static var file: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: SharedInbox.group)?
            .appendingPathComponent("glance.json")
    }

    static func read() -> Glance {
        guard let f = file, let d = try? Data(contentsOf: f), let g = try? JSONDecoder().decode(Glance.self, from: d) else {
            return .unknown
        }
        return g
    }

    /// True when what's kept changed, so the widgets are only asked to
    /// redraw when there's something new (WidgetKit budgets reloads).
    @discardableResult
    static func keep(_ g: Glance) -> Bool {
        guard let f = file else { return false }
        let old = read()
        // The timestamp moves every fetch; only a change in what shows counts.
        if old.status == g.status && old.home == g.home && old.lock == g.lock && old.as_of + 10 * 60 > g.as_of {
            return false
        }
        guard let d = try? JSONEncoder().encode(g) else { return false }
        try? d.write(to: f, options: .atomic)
        return true
    }
}
