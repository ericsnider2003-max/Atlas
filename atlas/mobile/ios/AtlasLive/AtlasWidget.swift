import ActivityKit
import SwiftUI
import WidgetKit

/// The card on the lock screen and the pill in the Dynamic Island. Status is
/// an icon and a word, never colour alone; everything has a spoken label.
@main
struct AtlasWidgets: WidgetBundle {
    var body: some Widget {
        AtlasLiveWidget()
        AtlasGlanceWidget()
    }
}

struct AtlasLiveWidget: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: AtlasActivity.self) { ctx in
            VStack(alignment: .leading, spacing: 6) {
                Label(ctx.state.doing.isEmpty ? "Ready for you" : "Working", systemImage: ctx.state.doing.isEmpty ? "checkmark.circle" : "clock")
                    .font(.headline)
                if !ctx.state.doing.isEmpty { Text(ctx.state.doing).font(.body) }
                if !ctx.state.step.isEmpty { Text(ctx.state.step).font(.footnote).foregroundStyle(.secondary) }
                if !ctx.state.ready.isEmpty {
                    Link(destination: URL(string: "atlas://hub/outstanding")!) {
                        Label(ctx.state.ready, systemImage: "tray.full")
                    }
                }
            }
            .padding()
            .accessibilityElement(children: .combine)
        } dynamicIsland: { ctx in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Label(ctx.state.doing.isEmpty ? "Ready" : "Working", systemImage: ctx.state.doing.isEmpty ? "checkmark.circle" : "clock")
                }
                DynamicIslandExpandedRegion(.bottom) {
                    Text(ctx.state.doing.isEmpty ? ctx.state.ready : ctx.state.step).lineLimit(2)
                }
            } compactLeading: {
                Image(systemName: ctx.state.doing.isEmpty ? "checkmark.circle" : "clock").accessibilityLabel(ctx.state.doing.isEmpty ? "Ready" : "Working")
            } compactTrailing: {
                Text(ctx.state.waiting > 0 ? "\(ctx.state.waiting)" : "").accessibilityLabel("\(ctx.state.waiting) waiting on you")
            } minimal: {
                Image(systemName: "clock").accessibilityLabel("Atlas")
            }
            .widgetURL(URL(string: "atlas://hub/now"))
        }
    }
}
