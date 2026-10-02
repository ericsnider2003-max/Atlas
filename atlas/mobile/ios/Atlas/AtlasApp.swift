import SwiftUI
import WebKit

@main
struct AtlasApp: App {
    @Environment(\.scenePhase) private var phase
    @StateObject private var model = HubModel()

    var body: some Scene {
        WindowGroup {
            Group {
                if let u = model.url {
                    HubView(url: u, model: model).ignoresSafeArea(.container, edges: .bottom)
                } else if model.failed {
                    // Said plainly, in the same words the hub uses.
                    Text("Atlas couldn't start on this phone. Its data is safe; try opening it again.")
                        .padding().multilineTextAlignment(.center)
                } else {
                    ProgressView("Starting Atlas…")
                }
            }
            .onOpenURL { model.open($0) }
            .task { await model.start() }
        }
        .onChange(of: phase) { p in
            if p == .active {
                Task { await model.wake() }
                CalendarSync.shared.maybeSync(force: true)
            }
            if p == .background { LiveActivity.shared.background() }
        }
    }
}

@MainActor
final class HubModel: ObservableObject {
    @Published var url: URL?
    @Published var failed = false
    weak var web: WKWebView?
    /// A link that arrived before the hub was answering: opened once it is.
    private var waitingLink: URL?
    private var delivering = false
    private var starting = false

    /// Started off the main thread (`AtlasCore.start`); the screen shows
    /// "Starting Atlas…" meanwhile instead of freezing.
    func start() async {
        guard !starting else { return }
        starting = true
        defer { starting = false }
        failed = false
        if await AtlasCore.shared.start() {
            url = AtlasCore.shared.hubURL
            LiveActivity.shared.begin()
            await takeShared()
        } else {
            failed = true
        }
    }

    /// Back in front: bring a hub that died in the background back, then
    /// pick up anything shared meanwhile.
    func wake() async {
        if url == nil { await start(); return }
        if await AtlasCore.shared.wake(), let u = AtlasCore.shared.hubURL {
            url = u
            web?.load(URLRequest(url: u))
        }
        if AtlasCore.shared.hubURL == nil { failed = true; url = nil; return }
        LiveActivity.shared.begin()
        await takeShared()
    }

    /// atlas://give?text=… or atlas://hub/<page>. Only ever *navigates*:
    /// any app can open an atlas:// link, so a link's words go into Give's
    /// box as a draft for you to send, and nothing else in its query is
    /// passed on (28 Sep 2026).
    func open(_ link: URL) {
        guard link.scheme == "atlas" else { return }
        // Not answering yet, or no web view to show it in: opened once there is.
        guard AtlasCore.shared.hubURL != nil, web != nil else { waitingLink = link; return }
        let comps = URLComponents(url: link, resolvingAgainstBaseURL: false)
        let words = comps?.queryItems?.first(where: { $0.name == "text" })?.value
        let page = link.host == "give" ? "/give" : link.path
        let safe = page.allSatisfy { $0.isLetter || $0.isNumber || $0 == "/" || $0 == "-" || $0 == "_" }
        var path = "/hub" + (safe ? page : "")
        if path.hasSuffix("/") { path.removeLast() }
        if path == "/hub/give", let w = words, !w.isEmpty {
            var c = URLComponents(); c.queryItems = [URLQueryItem(name: "draft", value: w)]
            path += "?" + (c.percentEncodedQuery ?? "")
        }
        if let u = AtlasCore.shared.url(path) { web?.load(URLRequest(url: u)) }
    }

    /// The web view exists: open a link that arrived before it did.
    func viewReady() {
        if let l = waitingLink { waitingLink = nil; open(l) }
    }

    /// What the share extension left goes to Atlas: each one handed over in
    /// turn, and removed only once Atlas has it (28 Sep 2026: they were
    /// deleted before the hub existed, and several shares replaced each
    /// other in the web view).
    func takeShared() async {
        guard !delivering, AtlasCore.shared.hubURL != nil else { return }
        delivering = true
        defer { delivering = false }
        var gave = 0
        for item in SharedInbox.pending() {
            guard await AtlasCore.shared.give(item.text) else { break }
            SharedInbox.remove(item.file)
            gave += 1
        }
        if gave > 0, let u = AtlasCore.shared.url("/hub/give") { web?.load(URLRequest(url: u)) }
    }
}

/// The hub, full screen. The phone design is the hub's own pages at phone
/// width (the tab bar, one column, safe areas), so everything the laptop
/// hub has, the phone has — and it follows the phone's text size, dark mode
/// and Reduce Motion because the pages read those preferences themselves.
struct HubView: UIViewRepresentable {
    let url: URL
    let model: HubModel

    func makeCoordinator() -> Shell { Shell() }

    func makeUIView(context: Context) -> WKWebView {
        let cfg = WKWebViewConfiguration()
        cfg.allowsInlineMediaPlayback = true
        let shell = context.coordinator
        cfg.userContentController.add(shell, name: "atlas")
        // window.AtlasShell: what the Talk page's hold-to-talk calls.
        let js = """
        window.AtlasShell={listen:function(){webkit.messageHandlers.atlas.postMessage({do:'listen'})},\
        stop:function(){webkit.messageHandlers.atlas.postMessage({do:'stop'})},\
        converse:function(){webkit.messageHandlers.atlas.postMessage({do:'converse'})},\
        speak:function(t){webkit.messageHandlers.atlas.postMessage({do:'speak',text:t})},\
        calendar:function(){webkit.messageHandlers.atlas.postMessage({do:'calendar'})}};
        """
        cfg.userContentController.addUserScript(WKUserScript(source: js, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        let w = WKWebView(frame: .zero, configuration: cfg)
        w.allowsBackForwardNavigationGestures = true
        w.isInspectable = false
        shell.web = w
        model.web = w
        w.load(URLRequest(url: url))
        DispatchQueue.main.async { model.viewReady() }
        return w
    }

    func updateUIView(_ w: WKWebView, context: Context) {}
}
