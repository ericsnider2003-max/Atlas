import Foundation
import Network

/// Atlas itself, running on this phone: the same Rust core as the laptop,
/// built for iOS, started once and kept for the life of the app.
final class AtlasCore {
    static let shared = AtlasCore()
    private(set) var hubURL: URL?
    /// The hub's token, for the app's own requests (live activity, share).
    private(set) var token: String?
    /// Wifi or not, told to Atlas: the phone's own model downloads by itself
    /// only on a network that isn't expensive (cellular) or constrained (Low Data Mode).
    private let paths = NWPathMonitor()

    /// The app's private folder. On first run the default config shipped in
    /// the bundle is copied into it; after that it is Atlas's own to change.
    private var home: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Atlas")
    }

    /// Start Atlas, off the main thread, and wait for its hub to answer.
    ///
    /// 28 Sep 2026: this used to call the core on the main actor and the core
    /// waited up to 20 s for the hub, freezing the "Starting Atlas…" screen;
    /// and a slow start left a second Atlas started next time. The core now
    /// returns at once (2: still starting) and starts only one; this polls
    /// for the address in the background.
    func start() async -> Bool {
        if hubURL != nil { return true }
        let home = self.home
        let url: String? = await Task.detached(priority: .userInitiated) { () -> String? in
            let fm = FileManager.default
            let cfg = home.appendingPathComponent("config")
            if !fm.fileExists(atPath: cfg.path), let shipped = Bundle.main.url(forResource: "config", withExtension: nil) {
                try? fm.createDirectory(at: home, withIntermediateDirectories: true)
                try? fm.copyItem(at: shipped, to: cfg)
            }
            let rc = home.path.withCString { atlas_mobile_start($0, 0) }
            guard rc >= 0 else { return nil }
            // Up to a minute: a first start on an old phone reads a lot.
            for _ in 0..<600 {
                var buf = [CChar](repeating: 0, count: 512)
                if atlas_mobile_url(&buf, buf.count) > 0 { return String(cString: buf) }
                if atlas_mobile_state() < 0 { return nil }
                try? await Task.sleep(nanoseconds: 100_000_000)
            }
            return nil
        }.value
        guard let s = url else { return false }
        // Apple's model as the first brain where this iPhone has it (decision 2),
        // and Apple's weather for weather answers.
        AppleBrain.register()
        AppleWeather.register()
        hubURL = URL(string: s)
        token = URLComponents(string: s)?.queryItems?.first(where: { $0.name == "t" })?.value
        if !watching {
            watching = true
            paths.pathUpdateHandler = { path in
                atlas_mobile_network(path.status == .satisfied && !path.isExpensive && !path.isConstrained ? 1 : 0)
            }
            paths.start(queue: DispatchQueue(label: "atlas.network"))
        }
        return hubURL != nil
    }
    private var watching = false

    /// Is the hub still answering? After the app has been in the background
    /// iOS may have taken its listening socket away while Atlas's own state
    /// still says it's running (28 Sep 2026: the hub was dead after a return
    /// from the background, and start() returned early because it had an
    /// address).
    func answering() async -> Bool {
        guard let u = url("/hub/live.json"), let t = token else { return false }
        var r = URLRequest(url: u, timeoutInterval: 2)
        r.setValue("Bearer \(t)", forHTTPHeaderField: "Authorization")
        guard let (_, resp) = try? await URLSession.shared.data(for: r) else { return false }
        return (resp as? HTTPURLResponse)?.statusCode == 200
    }

    /// Back from the background: if the hub doesn't answer, stop what's left
    /// of Atlas and start it again. Returns whether the address changed.
    func wake() async -> Bool {
        if hubURL == nil { return await start() }
        // Apple Intelligence may have been switched on or off, or finished
        // downloading, while the app was away.
        AppleBrain.register()
        if await answering() { return false }
        let before = hubURL
        hubURL = nil
        token = nil
        atlas_mobile_stop()
        // Wait (off the main thread) for the old one to finish its turn.
        await Task.detached {
            for _ in 0..<50 where atlas_mobile_state() == 2 { try? await Task.sleep(nanoseconds: 100_000_000) }
        }.value
        _ = await start()
        return hubURL != before
    }

    func stop() {
        hubURL = nil
        token = nil
        atlas_mobile_stop()
    }

    /// Hand text to Atlas, as the Give page's button does: a POST with the
    /// app's bearer. `true` once Atlas has it.
    func give(_ text: String) async -> Bool {
        guard let u = url("/hub/give"), let t = token else { return false }
        var r = URLRequest(url: u, timeoutInterval: 10)
        r.httpMethod = "POST"
        r.setValue("Bearer \(t)", forHTTPHeaderField: "Authorization")
        r.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        var c = URLComponents()
        c.queryItems = [URLQueryItem(name: "what", value: "hand"), URLQueryItem(name: "text", value: text)]
        // Form encoding: '+' must be sent as %2B, or it arrives as a space.
        r.httpBody = (c.percentEncodedQuery ?? "").replacingOccurrences(of: "+", with: "%2B").data(using: .utf8)
        guard let (_, resp) = try? await NoRedirect.session.data(for: r),
              let code = (resp as? HTTPURLResponse)?.statusCode else { return false }
        // The hub answers a form with a redirect back to the page.
        return (200..<400).contains(code)
    }

    /// An address on the hub, e.g. "/hub/give?text=…".
    func url(_ path: String) -> URL? {
        guard let base = hubURL, var c = URLComponents(url: base, resolvingAgainstBaseURL: false) else { return nil }
        let parts = path.split(separator: "?", maxSplits: 1).map(String.init)
        c.path = parts[0]
        c.percentEncodedQuery = parts.count > 1 ? parts[1] : nil
        return c.url
    }

    /// What Atlas is doing and what's ready — the same data the hub's Now page reads.
    func live() async -> LiveState? {
        guard let u = url("/hub/live.json"), let t = token else { return nil }
        var r = URLRequest(url: u)
        r.setValue("Bearer \(t)", forHTTPHeaderField: "Authorization")
        guard let (data, _) = try? await URLSession.shared.data(for: r) else { return nil }
        return try? JSONDecoder().decode(LiveState.self, from: data)
    }
}

extension AtlasCore {
    /// This phone's push address, to Atlas on this phone (`/hub/push-token`).
    /// Ad hoc and TestFlight builds use Apple's production push service.
    func keepPushAddress(_ hex: String) async {
        guard let u = url("/hub/push-token"), let t = token,
              let body = try? JSONSerialization.data(withJSONObject: ["token": hex, "env": "production"]) else { return }
        var r = URLRequest(url: u)
        r.httpMethod = "POST"
        r.setValue("Bearer \(t)", forHTTPHeaderField: "Authorization")
        r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        r.httpBody = body
        _ = try? await URLSession.shared.data(for: r)
    }

    /// The widgets' glance (/hub/glance.json), the same bearer as live().
    func glance() async -> Glance? {
        guard let u = url("/hub/glance.json"), let t = token else { return nil }
        var r = URLRequest(url: u)
        r.setValue("Bearer \(t)", forHTTPHeaderField: "Authorization")
        guard let (data, _) = try? await URLSession.shared.data(for: r) else { return nil }
        return try? JSONDecoder().decode(Glance.self, from: data)
    }
}

struct LiveState: Decodable {
    struct Working: Decodable { let title: String; let step: String; let stage: String }
    struct Ready: Decodable { let title: String; let href: String }
    /// What Atlas said in the background, numbered (reminders, finished work).
    struct Said: Decodable { let id: Int64; let text: String }
    /// A reminder still to come (item 15): handed to iOS on going to the
    /// background so it rings with the app closed.
    struct Upcoming: Decodable { let id: Int64; let due: Int64; let text: String }
    let status: String
    let working: Working?
    let ready: [Ready]
    let waiting: Int
    let said: [Said]?
    let upcoming: [Upcoming]?
}

/// A session that doesn't follow the hub's redirect after a form: the
/// redirect *is* the answer, and following it would drop the bearer.
final class NoRedirect: NSObject, URLSessionTaskDelegate {
    static let session = URLSession(configuration: .ephemeral, delegate: NoRedirect(), delegateQueue: nil)
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}
