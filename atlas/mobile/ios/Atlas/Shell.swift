import AVFoundation
import UIKit
import Speech
import WebKit

/// Listening and speaking, both on the phone. Speech recognition is asked to
/// stay on the device (`requiresOnDeviceRecognition`): if the phone can't do
/// that for the chosen language, hold-to-talk says so instead of sending audio
/// anywhere.
final class Shell: NSObject, WKScriptMessageHandler {
    weak var web: WKWebView?
    private let engine = AVAudioEngine()
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    private var heard = ""
    private let voice = AVSpeechSynthesizer()

    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        guard let body = m.body as? [String: Any], let what = body["do"] as? String else { return }
        switch what {
        case "listen": listen()
        case "stop": finish()
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
        try? session.setCategory(.playAndRecord, mode: .measurement, options: [.duckOthers, .defaultToSpeaker])
        try? session.setActive(true)
        let input = engine.inputNode
        input.installTap(onBus: 0, bufferSize: 1024, format: input.outputFormat(forBus: 0)) { b, _ in req.append(b) }
        try? engine.start()
        task = rec.recognitionTask(with: req) { r, _ in
            if let r { self.heard = r.bestTranscription.formattedString }
        }
    }

    private func finish() {
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
