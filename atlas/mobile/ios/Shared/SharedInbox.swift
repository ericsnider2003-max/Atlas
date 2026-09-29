import Foundation

/// The share extension can't run Atlas (extensions get seconds and little
/// memory), so it leaves what was shared in the app group's folder and the app
/// hands it to Give when it next comes forward. Nothing leaves the phone.
enum SharedInbox {
    static let group = "group.group.com.ericsnider.atlas"

    static var folder: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group)?.appendingPathComponent("inbox", isDirectory: true)
    }

    static func leave(_ text: String) {
        guard let f = folder else { return }
        try? FileManager.default.createDirectory(at: f, withIntermediateDirectories: true)
        try? text.write(to: f.appendingPathComponent(UUID().uuidString + ".txt"), atomically: true, encoding: .utf8)
    }

    /// What's waiting, oldest first, without taking it: each is removed only
    /// once Atlas has it (`remove`).
    static func pending() -> [(file: URL, text: String)] {
        guard let f = folder, let names = try? FileManager.default.contentsOfDirectory(at: f, includingPropertiesForKeys: [.creationDateKey]) else { return [] }
        let dated = names.map { u in (u, (try? u.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast) }
        return dated.sorted { $0.1 < $1.1 }.compactMap { pair -> (file: URL, text: String)? in
            guard let t = try? String(contentsOf: pair.0, encoding: .utf8) else { return nil }
            return (file: pair.0, text: t)
        }
    }

    static func remove(_ file: URL) {
        try? FileManager.default.removeItem(at: file)
    }
}
