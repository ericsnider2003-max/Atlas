import UIKit
import UniformTypeIdentifiers

/// "Share → Atlas": takes a link or words, leaves them for Atlas, and opens it.
final class ShareViewController: UIViewController {
    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        let items = (extensionContext?.inputItems as? [NSExtensionItem]) ?? []
        let providers = items.flatMap { $0.attachments ?? [] }
        let group = DispatchGroup()
        var parts: [String] = items.compactMap { $0.attributedContentText?.string }.filter { !$0.isEmpty }
        for p in providers {
            for t in [UTType.url, UTType.plainText] where p.hasItemConformingToTypeIdentifier(t.identifier) {
                group.enter()
                p.loadItem(forTypeIdentifier: t.identifier) { v, _ in
                    if let u = v as? URL { parts.append(u.absoluteString) } else if let s = v as? String { parts.append(s) }
                    group.leave()
                }
                break
            }
        }
        group.notify(queue: .main) {
            let text = parts.joined(separator: "\n")
            if !text.isEmpty { SharedInbox.leave(text) }
            self.extensionContext?.completeRequest(returningItems: nil)
        }
    }
}
