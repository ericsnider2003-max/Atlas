import AVFoundation
import UIKit
import Speech
import WebKit

/// Listening and speaking, both on the phone. Speech recognition is asked to
/// stay on the device (`requiresOnDeviceRecognition`): if the phone can't do
/// that for the chosen language, hold-to-talk says so instead of sending audio
/// anywhere.
///
/// Hands-free (2 Oct 2026, the "why stale" report, idea 10): `converse`
/// listens until you pause, sends what you said, and once Atlas has spoken
/// the answer the page asks it to listen again -- so a conversation runs
/// through AirPods away from the desk. Bluetooth headsets are allowed for
/// both the microphone and the voice.
final class Shell: NSObject, WKScriptMessageHandler, AVSpeechSynthesizerDelegate {
    weak var web: WKWebView?
    private let engine = AVAudioEngine()
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    private var heard = ""
    private let voice = AVSpeechSynthesizer()
    /// Listening ends itself after this long without a new word.
    private let pause: TimeInterval = 1.5
    /// And gives up after this long with no words at all.
    private let nothing: TimeInterval = 8
    private var conversing = false
    private var pauseTimer: Timer?
    private var startedAt = Date()

    override init() {
        super.init()
        voice.delegate = self
    }

    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        guard let body = m.body as? [String: Any], let what = body["do"] as? String else { return }
        switch what {
        case "listen": conversing = false; listen()
        case "converse": conversing = true; listen()
        case "stop": conversing = false; finish()
        case "speak": speak(body["text"] as? String ?? "")
        default: break
        }
    }

    private func listen() {
        SFSpeechRecognizer.requestAuthorization { ok in
            guard ok == .authorized else { return self.tell("Atlas needs permission to hear you. It's in Settings → Atlas.") }
            DispatchQueue.main.async { self.begin() }
        }
    }

    private func begin() {
        guard let rec = SFSpeechRecognizer(), rec.supportsOnDeviceRecognition else {
            return tell("This phone can't recognise speech on the device for this language, so Atlas won't listen — type instead.")
        }
        heard = ""
        let req = SFSpeechAudioBufferRecognitionRequest()
        req.requiresOnDeviceRecognition = true
        req.shouldReportPartialResults = true
        request = req
        let session = AVAudioSession.sharedInstance()
        // AirPods and other headsets: their microphone (HFP) and their
        // speaker (A2DP), with the phone's speaker when none is connected.
        try? session.setCategory(.playAndRecord, mode: .voiceChat, options: [.duckOthers, .defaultToSpeaker, .allowBluetooth, .allowBluetoothA2DP])
        try? session.setActive(true)
        let input = engine.inputNode
        input.installTap(onBus: 0, bufferSize: 1024, format: input.outputFormat(forBus: 0)) { b, _ in req.append(b) }
        try? engine.start()
        startedAt = Date()
        task = rec.recognitionTask(with: req) { r, _ in
            if let r {
                self.heard = r.bestTranscription.formattedString
                if self.conversing { DispatchQueue.main.async { self.armPause() } }
            }
        }
        if conversing { armPause() }
    }

    /// Hands-free: finish once you've paused, or give up if nothing came.
    private func armPause() {
        pauseTimer?.invalidate()
        let wait = heard.isEmpty ? nothing : pause
        pauseTimer = Timer.scheduledTimer(withTimeInterval: wait, repeats: false) { _ in
            if self.heard.isEmpty {
                self.conversing = false
                self.stopEngine()
                self.web?.evaluateJavaScript("window.atlasQuiet && window.atlasQuiet()")
            } else {
                self.finish()
            }
        }
    }

    private func stopEngine() {
        pauseTimer?.invalidate()
        if engine.isRunning {
            engine.stop()
            engine.inputNode.removeTap(onBus: 0)
        }
        request?.endAudio()
        task?.cancel()
    }

    /// Atlas has finished speaking: the page decides whether to listen again.
    func speechSynthesizer(_ s: AVSpeechSynthesizer, didFinish u: AVSpeechUtterance) {
        DispatchQueue.main.async {
            self.web?.evaluateJavaScript("window.atlasSpoke && window.atlasSpoke()")
        }
    }

    private func finish() {
        pauseTimer?.invalidate()
        guard engine.isRunning else { return }
        engine.stop()
        engine.inputNode.removeTap(onBus: 0)
        request?.endAudio()
        task?.finish()
        let said = heard
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
            let s = (said.isEmpty ? self.heard : said)
            guard !s.isEmpty, let json = try? JSONSerialization.data(withJSONObject: [s]),
                  let arg = String(data: json, encoding: .utf8) else { return }
            self.web?.evaluateJavaScript("window.atlasHeard && window.atlasHeard(\(arg)[0])")
        }
    }

    private func speak(_ text: String) {
        guard !text.isEmpty else { return }
        let u = AVSpeechUtterance(string: text)
        u.prefersAssistiveTechnologySettings = true
        voice.speak(u)
    }

    private func tell(_ s: String) {
        DispatchQueue.main.async {
            UIAccessibility.post(notification: .announcement, argument: s)
            guard let json = try? JSONSerialization.data(withJSONObject: [s]), let arg = String(data: json, encoding: .utf8) else { return }
            self.web?.evaluateJavaScript("(function(t){var n=document.getElementById('holdnote');if(n){n.textContent=t;n.setAttribute('role','status');}})(\(arg)[0])")
        }
    }
}
