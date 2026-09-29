import ActivityKit
import Foundation

/// Shared by the app (which starts and updates the activity) and the widget
/// extension (which draws it).
struct AtlasActivity: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        var status: String
        var doing: String
        var step: String
        var ready: String
        var waiting: Int
    }
}
