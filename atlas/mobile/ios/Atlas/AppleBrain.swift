import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

/// Apple's on-device model as Atlas's first brain on this iPhone (decision 2,
/// Eric's yes, 1 Oct 2026). The core decides which requests come here and
/// sends each one it refuses -- or can't do -- to Atlas's own model
/// (`src/applebrain.rs`). This side only answers, or says why it didn't:
///
///   0 answered   1 refused   2 too long   3 unavailable   4 failed
///
/// Nothing is kept between requests. Older iPhones and phones without Apple
/// Intelligence never register, so they always use Atlas's own model.
enum AppleBrain {
    /// Hand the core this phone's model if it's available now, or take it
    /// back if it isn't (switched off, still downloading, Low Power Mode).
    /// Called at start and each time the app comes forward.
    static func register() {
        #if canImport(FoundationModels)
        if #available(iOS 26.0, *), case .available = SystemLanguageModel.default.availability {
            atlas_mobile_apple_model(answer)
            return
        }
        #endif
        atlas_mobile_apple_model(nil)
    }

    /// The answer, written by the model's task and read once it's done
    /// (the semaphore orders the two).
    private final class AnswerBox: @unchecked Sendable {
        var code: Int32 = 4
        var text = ""
    }

    #if canImport(FoundationModels)
    /// Called by the core from its own thread: read the request, ask the
    /// model, write the answer. Waits for the answer (the core's thread
    /// is waiting for it anyway); never called on the main thread.
    private static let answer: atlas_apple_fn = { req, out, len in
        guard let req, let out, len > 1 else { return 4 }
        guard #available(iOS 26.0, *) else { return 3 }
        let data = Data(bytes: req, count: strlen(req))
        guard let body = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return 4 }
        let instructions = body["instructions"] as? String ?? ""
        let turns = body["turns"] as? [[String: String]] ?? []
        let most = body["max_tokens"] as? Int ?? 400
        // The conversation as one prompt, latest last: the session itself
        // is fresh for every request, so nothing leaks between them.
        var prompt = ""
        if turns.count > 1 {
            prompt += "The conversation so far:\n"
            for t in turns.dropLast() {
                let who = t["role"] == "assistant" ? "You" : "Them"
                prompt += "\(who): \(t["content"] ?? "")\n"
            }
            prompt += "\nAnswer this, the latest:\n"
        }
        prompt += turns.last?["content"] ?? ""

        let result = AnswerBox()
        let done = DispatchSemaphore(value: 0)
        Task.detached(priority: .userInitiated) {
            defer { done.signal() }
            do {
                let session = LanguageModelSession(instructions: instructions)
                let r = try await session.respond(to: prompt, options: GenerationOptions(maximumResponseTokens: max(64, min(most, 1024))))
                result.text = r.content
                result.code = 0
            } catch let e as LanguageModelSession.GenerationError {
                switch e {
                case .guardrailViolation: result.code = 1
                case .refusal: result.code = 1
                case .exceededContextWindowSize: result.code = 2
                case .assetsUnavailable: result.code = 3
                default: result.code = 4; result.text = "\(e)"
                }
            } catch {
                result.code = 4
                result.text = "\(error)"
            }
        }
        done.wait()
        let code = result.code
        let text = result.text
        let reply: Data
        if code == 0 {
            reply = (try? JSONSerialization.data(withJSONObject: ["text": text])) ?? Data()
        } else {
            reply = Data(text.utf8)
        }
        let n = min(reply.count, len - 1)
        reply.withUnsafeBytes { raw in
            if let base = raw.baseAddress { memcpy(out, base, n) }
        }
        out[n] = 0
        return code
    }
    #endif
}
