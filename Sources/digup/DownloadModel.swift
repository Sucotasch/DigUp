import DigUpKit
import Foundation

/// `digup download-model [--to DIR] [--check]`: the model files into DIR (default: DigUp's own Models folder), for
/// shells that set DigUp up on first launch (the Mac app downloads them itself, with the same `FileFetcher`). A file
/// already in the Hugging Face cache is copied from there; anything else is downloaded, picking up where an earlier try
/// stopped. Every file is checked (size, SHA-256) before it gets its name, so a file there is a good one. `--check`
/// hashes files that are already the right size and fetches the damaged ones again.
///
/// JSON lines on stdout:
///   {"event":"file","name":…,"bytes":n,"from":"cache|download"}             a file started
///   {"event":"progress","name":…,"bytes":n,"total":n,"all_bytes":n,"all_total":n}   at most 5 times a second
///   {"event":"waiting"}                                                     no network yet: it waits for one
///   {"event":"verifying","name":…}                                          the SHA-256 check (about a second)
///   {"event":"retry","message":…,"seconds":n}                               a failure it will try again after
///   {"event":"done","folder":…} | {"event":"error","message":…}             the end (exit 0 | 1)
func downloadModel(_ args: Arguments) throws -> Int32 {
    let folder = args.values["--to"].map(Arguments.folderURL) ?? ModelFiles.installed
    do {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    } catch {
        emit(["event": "error", "message": "Can't make the model folder: \(error.localizedDescription)"])
        return 1
    }
    let check = args.flags.contains("--check")
    var fetch: [ModelFiles.File] = []
    var done: Int64 = 0
    for file in ModelFiles.all {
        let state = ModelFiles.check(file, in: folder)
        var good = state.ok
        if good && check {
            emit(["event": "verifying", "name": file.name])
            good = (try? ModelFiles.sha256(of: folder.appendingPathComponent(file.name))) == file.sha256
        }
        if good {
            done += file.bytes
        } else {
            fetch.append(file)
        }
    }
    // The old file stays until its replacement is verified, so both need room at once. 200 MB is room to breathe.
    let old = fetch.reduce(Int64(0)) { sum, file in
        let size = (try? FileManager.default.attributesOfItem(atPath: folder.appendingPathComponent(file.name).path)[.size]
            as? NSNumber)?.int64Value ?? 0
        return sum + (size > 0 ? size : 0)
    }
    let needed = fetch.reduce(0) { $0 + $1.bytes } + old + (200 << 20)
    if let free = ModelFiles.freeSpace(at: folder), free < needed {
        emit(["event": "error", "message": "Not enough space in \(folder.path): "
              + "\(ByteCountFormatter.string(fromByteCount: needed, countStyle: .file)) needed, "
              + "\(ByteCountFormatter.string(fromByteCount: free, countStyle: .file)) free"])
        return 1
    }
    for file in fetch {
        let final = folder.appendingPathComponent(file.name)
        if let cached = ModelFiles.cachedCopy(of: file), (try? ModelFiles.sha256(of: cached)) == file.sha256 {
            emit(["event": "file", "name": file.name, "bytes": file.bytes, "from": "cache"])
            let partial = folder.appendingPathComponent(file.name + ".partial")
            do {
                try? FileManager.default.removeItem(at: partial)
                try FileManager.default.copyItem(at: cached, to: partial)
                emit(["event": "verifying", "name": file.name])
                guard try ModelFiles.sha256(of: partial) == file.sha256 else {
                    throw ModelFileError("\(file.name) arrived damaged")
                }
                try install(partial, as: final)
                done += file.bytes
                continue
            } catch {
                try? FileManager.default.removeItem(at: partial)
                // Fall through to the network: the cache copy wasn't usable.
                emit(["event": "retry", "message": "the cached copy couldn't be used: \(error.localizedDescription)", "seconds": 0])
            }
        }
        emit(["event": "file", "name": file.name, "bytes": file.bytes, "from": "download"])
        var failures = 0
        while true {
            switch fetchFile(file, into: folder, before: done) {
            case .downloaded(let partial):
                emit(["event": "verifying", "name": file.name])
                guard (try? ModelFiles.sha256(of: partial)) == file.sha256 else {
                    try? FileManager.default.removeItem(at: partial)
                    failures += 1
                    guard failures < 3 else {
                        emit(["event": "error", "message": "\(file.name) arrived damaged, three times"])
                        return 1
                    }
                    emit(["event": "retry", "message": "\(file.name) arrived damaged", "seconds": 0])
                    continue
                }
                do {
                    try install(partial, as: final)
                    done += file.bytes
                } catch {
                    emit(["event": "error", "message": "Can't put \(file.name) in place: \(error.localizedDescription)"])
                    return 1
                }
            case .failed(let message, let retry):
                failures += 1
                guard retry, failures <= 20 else {
                    emit(["event": "error", "message": message])
                    return 1
                }
                // A flaky link resumes where it stopped: 2, 4, 8 … seconds apart, a minute at most.
                let seconds = min(60, 1 << min(failures, 6))
                emit(["event": "retry", "message": message, "seconds": seconds])
                Thread.sleep(forTimeInterval: Double(seconds))
                continue
            }
            break
        }
    }
    emit(["event": "done", "folder": folder.path])
    return 0
}

/// `digup model-manifest`: the files, sizes, checksums and download addresses (one JSON object).
func modelManifest(_ args: Arguments) -> Int32 {
    emit(ModelFiles.manifest)
    return 0
}

/// `digup model-verify --to DIR`: size and SHA-256 for every model file.
///   {"event":"file","name":…,"bytes":n,"present":true,"size":n,"ok":true,"problem":null}
///   {"event":"done","ok":true,"folder":…,"checked":2}
func modelVerify(_ args: Arguments) -> Int32 {
    guard let folder = args.values["--to"].map(Arguments.folderURL) else {
        emit(["event": "error", "message": "model-verify needs --to DIR"])
        return 2
    }
    var allOK = true
    for file in ModelFiles.all {
        let state = ModelFiles.check(file, in: folder, hash: true)
        allOK = allOK && state.ok
        emit(["event": "file", "name": file.name, "bytes": file.bytes, "present": state.present,
              "size": state.size, "ok": state.ok, "problem": state.problem.map { $0 as Any } ?? NSNull()])
    }
    emit(["event": "done", "ok": allOK, "folder": folder.path, "checked": ModelFiles.all.count])
    return allOK ? 0 : 1
}

/// `digup model-import --to DIR FILE...`: copies model files chosen from other folders into DIR (a `.partial` while it
/// copies, its SHA-256 checked, then its name). Files that aren't one of the model's two names are refused before
/// anything is changed. A file already at its destination is verified in place, not copied.
///   {"event":"file","name":…,"bytes":n,"from":path}
///   {"event":"progress","name":…,"bytes":n,"total":n,"all_bytes":n,"all_total":n}
///   {"event":"verifying","name":…}
///   {"event":"done","folder":…} | {"event":"error","message":…}
func modelImport(_ args: Arguments) -> Int32 {
    guard let folder = args.values["--to"].map(Arguments.folderURL) else {
        emit(["event": "error", "message": "model-import needs --to DIR"])
        return 2
    }
    let sources = args.positional.map(Arguments.folderURL)
    guard !sources.isEmpty else {
        emit(["event": "error", "message": "which files?"])
        return 2
    }
    var chosen: [(file: ModelFiles.File, source: URL)] = []
    for source in sources {
        let name = source.lastPathComponent
        guard let file = ModelFiles.all.first(where: { $0.name == name }) else {
            emit(["event": "error", "message": "\(name) isn't a DigUp model file"])
            return 1
        }
        guard !chosen.contains(where: { $0.file.name == name }) else {
            emit(["event": "error", "message": "\(name) was chosen twice"])
            return 1
        }
        chosen.append((file, source))
    }
    // Everything is read and checked before anything is replaced, so a bad file leaves the folder as it was.
    var inPlace = 0
    for (file, source) in chosen {
        guard FileManager.default.fileExists(atPath: source.path) else {
            emit(["event": "error", "message": "\(source.path) isn't there"])
            return 1
        }
        let size = (try? FileManager.default.attributesOfItem(atPath: source.path)[.size] as? NSNumber)?.int64Value ?? 0
        guard size == file.bytes else {
            emit(["event": "error", "message": "\(file.name) is \(ByteCountFormatter.string(fromByteCount: size, countStyle: .file)), "
                  + "not \(ByteCountFormatter.string(fromByteCount: file.bytes, countStyle: .file))"])
            return 1
        }
        emit(["event": "verifying", "name": file.name])
        guard (try? ModelFiles.sha256(of: source)) == file.sha256 else {
            emit(["event": "error", "message": "\(file.name) doesn't match its checksum"])
            return 1
        }
        if source.standardizedFileURL == folder.appendingPathComponent(file.name).standardizedFileURL {
            inPlace += 1
        }
    }
    do {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    } catch {
        emit(["event": "error", "message": "Can't make the model folder: \(error.localizedDescription)"])
        return 1
    }
    let copying = chosen.filter { $0.source.standardizedFileURL != folder.appendingPathComponent($0.file.name).standardizedFileURL }
    // A file being replaced stays until its verified copy is ready, so both need room at once.
    let replacing = copying.reduce(Int64(0)) { sum, item in
        let size = (try? FileManager.default.attributesOfItem(
            atPath: folder.appendingPathComponent(item.file.name).path)[.size] as? NSNumber)?.int64Value ?? 0
        return sum + max(0, size)
    }
    let needed = copying.reduce(Int64(0)) { $0 + $1.file.bytes } + replacing + (200 << 20)
    if let free = ModelFiles.freeSpace(at: folder), free < needed {
        emit(["event": "error", "message": "Not enough space in \(folder.path): "
              + "\(ByteCountFormatter.string(fromByteCount: needed, countStyle: .file)) needed"])
        return 1
    }
    let allTotal = chosen.reduce(Int64(0)) { $0 + $1.file.bytes }
    var done = Int64(0)
    for (file, source) in chosen {
        let final = folder.appendingPathComponent(file.name)
        guard source.standardizedFileURL != final.standardizedFileURL else {
            done += file.bytes
            emit(["event": "progress", "name": file.name, "bytes": file.bytes, "total": file.bytes,
                  "all_bytes": done, "all_total": allTotal])
            continue
        }
        emit(["event": "file", "name": file.name, "bytes": file.bytes, "from": source.path])
        let partial = folder.appendingPathComponent(file.name + ".partial")
        do {
            try? FileManager.default.removeItem(at: partial)
            try copy(source, to: partial, file: file, before: done, allTotal: allTotal)
            guard (try? ModelFiles.sha256(of: partial)) == file.sha256 else {
                throw ModelFileError("\(file.name) arrived damaged")
            }
            emit(["event": "verifying", "name": file.name])
            try install(partial, as: final)
            done += file.bytes
        } catch {
            try? FileManager.default.removeItem(at: partial)
            emit(["event": "error", "message": "\(file.name) couldn't be copied: \(error.localizedDescription)"])
            return 1
        }
    }
    emit(["event": "done", "folder": folder.path, "in_place": inPlace])
    return 0
}

private struct ModelFileError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

/// Replaces `final` with `partial` without a moment without a model: the old file steps aside (Windows won't rename
/// over it), the partial takes its name, and only then the old one goes. A move that fails (a scanner or a backup
/// holding the file) restores the old file, so the model is never the casualty. A folder with the model's name is
/// never removed: it may hold the person's own files.
private func install(_ partial: URL, as final: URL) throws {
    var isFolder: ObjCBool = false
    let exists = FileManager.default.fileExists(atPath: final.path, isDirectory: &isFolder)
    guard !(exists && isFolder.boolValue) else {
        throw ModelFileError("\(final.lastPathComponent) is a folder; move it out of the way and try again")
    }
    let old = final.appendingPathExtension("old")
    try? FileManager.default.removeItem(at: old)   // one left behind by an interrupted install
    if exists {
        try FileManager.default.moveItem(at: final, to: old)
    }
    do {
        try FileManager.default.moveItem(at: partial, to: final)
    } catch {
        if exists { try? FileManager.default.moveItem(at: old, to: final) }
        throw error
    }
    try? FileManager.default.removeItem(at: old)
}

/// Copies a file in 8 MB pieces, saying how far it got at most five times a second.
private func copy(_ source: URL, to partial: URL, file: ModelFiles.File, before done: Int64, allTotal: Int64) throws {
    guard FileManager.default.createFile(atPath: partial.path, contents: nil) else {
        throw ModelFileError("Can't make \(partial.lastPathComponent)")
    }
    let input = try FileHandle(forReadingFrom: source)
    let output = try FileHandle(forWritingTo: partial)
    defer {
        try? input.close()
        try? output.close()
    }
    var written: Int64 = 0
    var reported = Date.distantPast
    while true {
        let chunk = try autoreleasepool { try input.read(upToCount: 8 << 20) }
        guard let chunk, !chunk.isEmpty else { break }
        try output.write(contentsOf: chunk)
        written += Int64(chunk.count)
        let now = Date()
        if now.timeIntervalSince(reported) >= 0.2 {
            reported = now
            emit(["event": "progress", "name": file.name, "bytes": written, "total": file.bytes,
                  "all_bytes": done + written, "all_total": allTotal])
        }
    }
    try output.synchronize()
}

/// One try at one file: blocks until `FileFetcher` is through with it.
private func fetchFile(_ file: ModelFiles.File, into folder: URL, before done: Int64) -> FileFetcher.Outcome {
    let finished = DispatchSemaphore(value: 0)
    let result = OutcomeBox()
    let fetcher = FileFetcher(file: file, folder: folder, progress: { written in
        emit(["event": "progress", "name": file.name, "bytes": written, "total": file.bytes,
              "all_bytes": done + written, "all_total": ModelFiles.totalBytes])
    }, waiting: {
        emit(["event": "waiting"])
    }, done: { outcome in
        result.outcome = outcome
        finished.signal()
    })
    fetcher.start()
    finished.wait()
    return result.outcome ?? .failed("the download stopped", retry: true)
}

private final class OutcomeBox: @unchecked Sendable {   // written once, before the semaphore is signalled
    var outcome: FileFetcher.Outcome?
}
