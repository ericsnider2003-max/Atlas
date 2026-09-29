import SwiftUI
import WidgetKit

/// Atlas at a glance, on the home screen and the lock screen: what it's doing
/// or what's next, how many things wait on you, and one tap to give it
/// something. Read from the glance the app leaves in the app group
/// (Shared/Glance.swift); the widget never runs Atlas or holds its token.
///
/// The lock-screen families read `lock`, which carries times and counts only
/// unless you turn on "titles on the lock screen" in Atlas; the home-screen
/// families read `home`. Status is an icon and a word, never colour alone,
/// and every element has a spoken label.
struct AtlasGlanceWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "AtlasGlance", provider: GlanceProvider()) { entry in
            GlanceView(glance: entry.glance)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Atlas")
        .description("What's next, what's waiting on you, and a tap to give Atlas something.")
        .supportedFamilies([.systemSmall, .systemMedium, .accessoryRectangular, .accessoryCircular, .accessoryInline])
    }
}

struct GlanceEntry: TimelineEntry {
    let date: Date
    let glance: Glance
}

struct GlanceProvider: TimelineProvider {
    func placeholder(in context: Context) -> GlanceEntry {
        GlanceEntry(date: Date(), glance: .unknown)
    }

    func getSnapshot(in context: Context, completion: @escaping (GlanceEntry) -> Void) {
        completion(GlanceEntry(date: Date(), glance: GlanceStore.read()))
    }

    /// One entry now and one when it turns stale, so an old glance changes
    /// to "as of …" by itself even if the app hasn't run since. The app asks
    /// for a reload whenever what shows changes.
    func getTimeline(in context: Context, completion: @escaping (Timeline<GlanceEntry>) -> Void) {
        let g = GlanceStore.read()
        let now = Date()
        var entries = [GlanceEntry(date: now, glance: g)]
        let stale = Date(timeIntervalSince1970: TimeInterval(g.as_of) + 15 * 60 + 1)
        if stale > now { entries.append(GlanceEntry(date: stale, glance: g)) }
        completion(Timeline(entries: entries, policy: .after(now.addingTimeInterval(30 * 60))))
    }
}

struct GlanceView: View {
    @Environment(\.widgetFamily) private var family
    let glance: Glance

    private var locked: Bool {
        [.accessoryRectangular, .accessoryCircular, .accessoryInline].contains(family)
    }
    private var view: Glance.View { locked ? glance.lock : glance.home }

    /// The same icons the hub's status pill uses for the same tones.
    private var statusIcon: String {
        switch glance.tone {
        case "held": return "pause.circle"
        case "off": return "moon.zzz"
        default: return view.working != nil ? "clock" : "checkmark.circle"
        }
    }

    /// "Working: …", "Next at 14:30: …", or the status when there's neither.
    private var headline: String {
        if let w = view.working { return w == "Working" ? "Working" : w }
        if let n = view.next { return n.what.isEmpty ? "Next at \(n.at)" : "\(n.at) \(n.what)" }
        return glance.status
    }

    private var waitingText: String {
        switch view.waiting {
        case 0: return "Nothing waiting"
        case 1: return "1 thing waiting"
        default: return "\(view.waiting) things waiting"
        }
    }

    private var capture: URL { URL(string: glance.capture) ?? URL(string: "atlas://hub/give")! }

    var body: some View {
        switch family {
        case .accessoryInline:
            Label(view.waiting > 0 ? "\(headline) · \(view.waiting) waiting" : headline, systemImage: statusIcon)
                .widgetURL(URL(string: "atlas://hub/now"))
        case .accessoryCircular:
            ZStack {
                AccessoryWidgetBackground()
                VStack(spacing: 0) {
                    Text("\(view.waiting)").font(.title2.weight(.semibold))
                    Text("waiting").font(.caption2)
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(waitingText)
            .widgetURL(URL(string: "atlas://hub/outstanding"))
        case .accessoryRectangular:
            VStack(alignment: .leading, spacing: 2) {
                Label(glance.status, systemImage: statusIcon).font(.headline)
                Text(headline).lineLimit(1)
                Text(glance.isStale ? "\(waitingText) · as of \(glance.asOf)" : waitingText)
                    .font(.caption).foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)
            .widgetURL(URL(string: "atlas://hub/now"))
        case .systemMedium:
            HStack(alignment: .top, spacing: 12) {
                details
                Spacer(minLength: 0)
                Link(destination: capture) {
                    Label("Give Atlas something", systemImage: "plus.bubble")
                        .labelStyle(.iconOnly).font(.title)
                        .frame(minWidth: 44, minHeight: 44)
                }
                .accessibilityLabel("Give Atlas something")
            }
            .widgetURL(URL(string: "atlas://hub/now"))
        default:
            details.widgetURL(URL(string: "atlas://hub/now"))
        }
    }

    private var details: some View {
        VStack(alignment: .leading, spacing: 4) {
            Label(glance.status, systemImage: statusIcon).font(.caption.weight(.semibold))
            Text(headline).font(.headline).lineLimit(3)
            Spacer(minLength: 0)
            Label(waitingText, systemImage: "tray").font(.caption)
            if glance.isStale && glance.as_of > 0 {
                Text("As of \(glance.asOf)").font(.caption2).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}
