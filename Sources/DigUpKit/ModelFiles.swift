#if canImport(CryptoKit)
import CryptoKit
#endif
import Foundation

/// The EmbeddingGemma 2 files the app runs: ggml-org's Q8_0 GGUF conversion at one pinned revision of the repo (made
/// from google/embeddinggemma-2 at 914f7f8, the weights the mlx-vlm reference runs). Never the F16 files: the model's
/// activations overflow float16.
///
/// The app downloads them on first launch (`ModelDownload`) and the helper loads them (`LlamaModels`); this is what
/// both agree on.
public enum ModelFiles {
    public static let repository = "ggml-org/embeddinggemma-2-GGUF"
    public static let revision = "bfcd298762cc34d0357ece5ebdd31791a3a374d8"

    public struct File: Sendable, Equatable {
        public let name: String
        public let bytes: Int64
        public let sha256: String

        public init(name: String, bytes: Int64, sha256: String) {
            (self.name, self.bytes, self.sha256) = (name, bytes, sha256)
        }

        /// Hugging Face's address for this file at the pinned revision (it redirects to a CDN that takes HTTP ranges).
        public var url: URL {
            URL(string: "https://huggingface.co/\(ModelFiles.repository)/resolve/\(ModelFiles.revision)/\(name)")!
        }
    }

    /// What one file looks like in a folder: there or not, its size, and whether it is the right bytes.
    public struct Check: Sendable, Equatable {
        public let file: File
        /// The name is in the folder, whatever its size.
        public let present: Bool
        public let size: Int64
        public let ok: Bool
        /// Why it isn't usable: "missing", "wrong size", "checksum mismatch", "not a file", "unreadable: …".
        public let problem: String?
    }

    public static let missingProblem = "missing"
    public static let wrongSizeProblem = "wrong size"
    public static let checksumProblem = "checksum mismatch"

    /// The text model (310 MB): all that queries need.
    public static let text = File(name: "embeddinggemma-2-Q8_0.gguf", bytes: 309_855_456,
                                  sha256: "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135")
    /// The image and audio encoders (555 MB).
    public static let media = File(name: "mmproj-embeddinggemma-2-Q8_0.gguf", bytes: 554_821_024,
                                   sha256: "c4a8a52691ecef40618438928bdf9e68379b854e24166f292592353db0aab64f")
    /// In download order.
    public static let all = [text, media]
    public static let totalBytes = all.reduce(0) { $0 + $1.bytes }

    /// Where the app keeps them (`AppPaths`). The shell passes its own folder with `--to`/`DIGUP_MODELS`; this is what
    /// the CLI and the Mac app use when they aren't told.
    public static let installed = AppPaths.applicationSupport.appendingPathComponent("Models")

    /// The pinned revision's folder in the Hugging Face cache (`hf download` puts the files there): `$HF_HUB_CACHE`,
    /// `$HF_HOME/hub`, or `~/.cache/huggingface/hub`.
    public static var huggingFaceSnapshot: URL {
        let environment = ProcessInfo.processInfo.environment
        let hub = environment["HF_HUB_CACHE"].map { URL(fileURLWithPath: $0) }
            ?? environment["HF_HOME"].map { URL(fileURLWithPath: $0).appendingPathComponent("hub") }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".cache/huggingface/hub")
        return hub.appendingPathComponent("models--\(repository.replacingOccurrences(of: "/", with: "--"))/snapshots/\(revision)")
    }

    /// `file` in the Hugging Face cache, when it's there at the right size (its checksum is for the caller to check).
    public static func cachedCopy(of file: File) -> URL? {
        let url = huggingFaceSnapshot.appendingPathComponent(file.name).resolvingSymlinksInPath()
        let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?.int64Value
        return size == file.bytes ? url : nil
    }

    /// One file in `folder`: its size, and with `hash` the SHA-256 too. A file with the right name but the wrong size
    /// is present and not usable; a folder with that name is not a file.
    public static func check(_ file: File, in folder: URL, hash: Bool = false) -> Check {
        let url = folder.appendingPathComponent(file.name)
        var isFolder: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isFolder) else {
            return Check(file: file, present: false, size: 0, ok: false, problem: missingProblem)
        }
        guard !isFolder.boolValue else {
            return Check(file: file, present: true, size: 0, ok: false, problem: "not a file")
        }
        let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?.int64Value ?? 0
        guard size == file.bytes else {
            return Check(file: file, present: true, size: size, ok: false, problem: wrongSizeProblem)
        }
        if hash {
            do {
                guard try sha256(of: url) == file.sha256 else {
                    return Check(file: file, present: true, size: size, ok: false, problem: checksumProblem)
                }
            } catch {
                return Check(file: file, present: true, size: size, ok: false,
                             problem: "unreadable: \(error.localizedDescription)")
            }
        }
        return Check(file: file, present: true, size: size, ok: true, problem: nil)
    }

    /// The files that aren't in `folder` or whose size is wrong. Only verified files get their final name, so size is
    /// enough here; `verify` checks the hashes.
    public static func missing(in folder: URL) -> [File] {
        all.filter { !check($0, in: folder).ok }
    }

    /// Both files are there at the right sizes (no hashing): indexing can run.
    public static func complete(in folder: URL) -> Bool {
        all.allSatisfy { check($0, in: folder).ok }
    }

    /// Every file's size and SHA-256. A missing file costs nothing; a whole model takes about a second.
    public static func verify(in folder: URL) -> [Check] {
        all.map { check($0, in: folder, hash: true) }
    }

    /// The free space at `folder`, or at its nearest folder that exists. Nil where the system doesn't say.
    public static func freeSpace(at folder: URL) -> Int64? {
        var existing = folder
        while !FileManager.default.fileExists(atPath: existing.path) {
            let parent = existing.deletingLastPathComponent()
            if parent.path == existing.path { return nil }
            existing = parent
        }
        guard let values = try? FileManager.default.attributesOfFileSystem(forPath: existing.path) else { return nil }
        return (values[.systemFreeSize] as? NSNumber)?.int64Value
    }

    /// What `model-manifest` prints: names, sizes, checksums and the pinned download addresses.
    public static var manifest: [String: Any] {
        ["files": all.map { ["name": $0.name, "bytes": $0.bytes, "sha256": $0.sha256, "url": $0.url.absoluteString] },
         "total": totalBytes]
    }

    /// The SHA-256 of a file as lowercase hex, read in 8 MB pieces (a 555 MB file takes about half a second).
    public static func sha256(of url: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hasher = SHA256()
        while true {
            let done = try autoreleasepool { () -> Bool in
                guard let data = try handle.read(upToCount: 8 << 20), !data.isEmpty else { return true }
                hasher.update(data: data)
                return false
            }
            if done { break }
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }
}
