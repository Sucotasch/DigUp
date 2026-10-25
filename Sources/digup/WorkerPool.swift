import DigUpKit
import Foundation

/// Several `digup worker` processes, each with its own llama.cpp context. One context leaves a fast GPU mostly idle
/// (measured: 32% on an RTX 4070 Ti SUPER); four of them reach ~89% and about twice the throughput, with identical
/// vectors. Calls are synchronous, so a worker is used by one thread at a time; text queries stay on the first worker.
final class WorkerPool: Embedder, @unchecked Sendable {
    private let workers: [EmbeddingWorker]
    private let locks: [NSLock]
    let info: WorkerInfo

    init(count: Int, executable: URL, log: URL?) throws {
        precondition(count > 1)
        // Starting the workers one after another would cost `count` model loads in a row; they load in parallel.
        final class Startup: @unchecked Sendable {
            let lock = NSLock()
            var workers: [EmbeddingWorker?]
            var failure: Error?
            init(_ count: Int) { workers = Array(repeating: nil, count: count) }
        }
        let startup = Startup(count)
        let group = DispatchGroup()
        for index in 0..<count {
            group.enter()
            DispatchQueue.global(qos: .utility).async {
                defer { group.leave() }
                var config = WorkerConfig.helper(executable)
                config.qos = .utility
                if let log {
                    let stem = log.deletingPathExtension().lastPathComponent
                    let ext = log.pathExtension
                    config.log = log.deletingLastPathComponent()
                        .appendingPathComponent("\(stem)-\(index).\(ext)")
                }
                do {
                    let worker = try EmbeddingWorker(config, textOnly: false)
                    startup.lock.withLock { startup.workers[index] = worker }
                } catch {
                    startup.lock.withLock { if startup.failure == nil { startup.failure = error } }
                }
            }
        }
        group.wait()
        if let failure = startup.failure {
            for worker in startup.workers.compactMap({ $0 }) { worker.close() }
            throw failure
        }
        let workers = startup.workers.compactMap { $0 }
        guard workers.count == count else {
            for worker in workers { worker.close() }
            throw WorkerError.failed("only \(workers.count) of \(count) embedding workers started")
        }
        self.workers = workers
        locks = workers.map { _ in NSLock() }
        info = workers[0].info
    }

    func embedTexts(_ texts: [String]) throws -> [[Float]] {
        guard !texts.isEmpty else { return [] }
        locks[0].lock()
        defer { locks[0].unlock() }
        return try workers[0].embedTexts(texts)
    }

    func embedImages(_ paths: [String], budget: Int) throws -> [Embedding] {
        guard !paths.isEmpty else { return [] }
        return try spread(paths) { worker, chunk in
            try worker.embedImages(chunk, budget: budget)
        }
    }

    func embedAudio(_ paths: [String]) throws -> [Embedding] {
        guard !paths.isEmpty else { return [] }
        return try spread(paths) { worker, chunk in
            try worker.embedAudio(chunk)
        }
    }

    /// Quits every worker at once: each spends seconds freeing its model, and one after another that is seconds per
    /// worker on the way to pausing or quitting.
    func close() {
        let group = DispatchGroup()
        for index in workers.indices {
            group.enter()
            DispatchQueue.global(qos: .utility).async {
                self.workers[index].close()
                group.leave()
            }
        }
        group.wait()
    }

    /// Splits the inputs into contiguous pieces, one per busy worker, and joins their results in the original order.
    private func spread(_ inputs: [String],
                        _ call: @escaping @Sendable (EmbeddingWorker, [String]) throws -> [Embedding]) throws -> [Embedding] {
        final class Box: @unchecked Sendable {
            let lock = NSLock()
            var results: [Embedding?]
            var failure: Error?
            init(_ count: Int) { results = Array(repeating: nil, count: count) }
        }
        let count = min(workers.count, inputs.count)
        let base = inputs.count / count
        let extra = inputs.count % count
        var built: [(offset: Int, items: [String])] = []
        var offset = 0
        for index in 0..<count {
            let size = base + (index < extra ? 1 : 0)
            built.append((offset, Array(inputs[offset..<offset + size])))
            offset += size
        }
        let parts = built
        let box = Box(inputs.count)
        let group = DispatchGroup()
        for index in 0..<count {
            group.enter()
            DispatchQueue.global(qos: .utility).async {
                defer { group.leave() }
                self.locks[index].lock()
                do {
                    let got = try call(self.workers[index], parts[index].items)
                    self.locks[index].unlock()
                    box.lock.withLock {
                        for (i, embedding) in got.enumerated() where parts[index].offset + i < box.results.count {
                            box.results[parts[index].offset + i] = embedding
                        }
                    }
                } catch {
                    self.locks[index].unlock()
                    box.lock.withLock { if box.failure == nil { box.failure = error } }
                }
            }
        }
        group.wait()
        if let failure = box.failure { throw failure }
        return box.results.map { $0 ?? Embedding(vector: nil, error: "no vector") }
    }
}
