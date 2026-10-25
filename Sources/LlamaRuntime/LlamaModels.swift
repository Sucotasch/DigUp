import DigUpKit
import Foundation

/// Where the EmbeddingGemma 2 files for llama.cpp are (see `ModelFiles` for which files, and why Q8_0).
public struct LlamaModels: Sendable {
    public static let repository = ModelFiles.repository
    public static let revision = ModelFiles.revision
    /// The text part (310 MB): all that queries need.
    public static let textFile = ModelFiles.text.name
    /// The image and audio encoders (555 MB).
    public static let mediaFile = ModelFiles.media.name

    public let text: URL
    /// Nil while only the text part is there.
    public let media: URL?

    /// The folders to look in, first match wins. `$DIGUP_MODELS` is the app's choice and the only place it looks
    /// then: the developer's Hugging Face cache mustn't hide a download that's missing. Without it (the CLI):
    /// Application Support, then the Hugging Face cache.
    public static var folders: [URL] {
        if let chosen = ProcessInfo.processInfo.environment["DIGUP_MODELS"], !chosen.isEmpty {
            return [URL(fileURLWithPath: chosen)]
        }
        return [ModelFiles.installed, ModelFiles.huggingFaceSnapshot]
    }

    /// The first folder with the text part. Missing files are a `WorkerError`, so an indexing run stops and its files
    /// stay pending.
    public static func locate() throws -> LlamaModels {
        for folder in folders {
            // A file with the right name but the wrong size is a broken download, not a model: leave it for
            // `download-model` to replace rather than handing it to llama.cpp.
            let text = ModelFiles.check(ModelFiles.text, in: folder)
            guard text.ok else { continue }
            let media = ModelFiles.check(ModelFiles.media, in: folder)
            return LlamaModels(text: folder.appendingPathComponent(textFile),
                               media: media.ok ? folder.appendingPathComponent(mediaFile) : nil)
        }
        throw WorkerError.notFound("no \(textFile) in " + folders.map(\.path).joined(separator: ", ")
            + "; get it with: hf download \(repository) \(textFile) \(mediaFile) --revision \(revision)")
    }

    /// Both files are there, so indexing can run (checked without loading anything).
    public static var complete: Bool {
        (try? locate())?.media != nil
    }
}
