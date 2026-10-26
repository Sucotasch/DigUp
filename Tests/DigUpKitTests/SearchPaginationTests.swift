import Foundation
import Testing
@testable import DigUpKit

@Suite final class SearchPaginationTests {
    let folder: URL = {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("DigUpPagingTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }()

    deinit { try? FileManager.default.removeItem(at: folder) }

    private func index(count: Int) throws -> Searcher {
        let store = try IndexStore(directory: folder)
        for number in 0..<count {
            let id = try store.insert(Candidate(path: "/paging/\(number).txt", kind: .doc, size: 1, mtime: 1,
                                                inode: UInt64(number + 1), device: 1))
            try store.addSegment(file: id, kind: .chunk, modality: .text, loc: 1,
                                 text: "paginationword item \(number)")
        }
        return try Searcher(store: store, loadVectors: false)
    }

    /// Meaning-only results: every file has the same vector, so with `zGate` 0 all of them are candidates. Keyword
    /// search alone stops at 100 files, which is too few to exercise a window-sized page.
    private func vectorIndex(count: Int) throws -> (Searcher, [Float]) {
        let store = try IndexStore(directory: folder)
        var vector = [Float](repeating: 0, count: Vectors.dimension)
        vector[0] = 1
        for number in 0..<count {
            let id = try store.insert(Candidate(path: "/paging/\(number).txt", kind: .doc, size: 1, mtime: 1,
                                                inode: UInt64(number + 1), device: 1))
            try store.addSegment(file: id, kind: .chunk, modality: .text, loc: 1, vector: vector)
        }
        return (try Searcher(store: store), vector)
    }

    @Test func pagesTileTheWholeRankingWithoutGapsOrDuplicates() throws {
        let searcher = try index(count: 7)
        let all = try searcher.search("paginationword", queryVector: nil, limit: 100)
        #expect(all.count == 7)
        let strong = Searcher.strongCount(all)

        var combined: [String] = []
        var offset = 0
        for _ in 0..<5 {
            let page = try searcher.page("paginationword", queryVector: nil, limit: 2, offset: offset)
            #expect(page.total == all.count)
            #expect(page.strong == strong)
            #expect(page.topScore == all.first?.score)
            #expect(page.offset == min(offset, all.count))
            #expect(page.consumed == min(2, max(0, all.count - offset)))
            combined += page.hits.map(\.path)
            offset += page.consumed
            if page.consumed == 0 { break }
        }
        #expect(combined == all.map(\.path))
        #expect(Set(combined).count == combined.count)
    }

    @Test func aThousandResultsTileAcrossTheWindowSizes() throws {
        let count = 1_000
        let (searcher, vector) = try vectorIndex(count: count)
        let all = try searcher.search("paginationword", queryVector: vector, limit: count, zGate: 0)
        #expect(all.count == count)
        var combined: [String] = []
        var offset = 0
        while offset < count {
            let limit = offset == 0 ? 120 : 300
            let page = try searcher.page("paginationword", queryVector: vector, limit: limit, offset: offset,
                                         zGate: 0)
            #expect(page.total == count)
            #expect(page.offset == offset)
            #expect(page.consumed == min(limit, count - offset))
            combined += page.hits.map(\.path)
            guard page.consumed > 0 else { break }
            offset += page.consumed
        }
        #expect(combined == all.map(\.path))
        #expect(Set(combined).count == combined.count)
    }

    @Test func offsetsAreClampedAndPastTheEndIsEmpty() throws {
        let searcher = try index(count: 3)
        let beyond = try searcher.page("paginationword", queryVector: nil, limit: 10, offset: 99)
        #expect(beyond.hits.isEmpty)
        #expect(beyond.total == 3)
        #expect(beyond.offset == 3)
        #expect(beyond.consumed == 0)

        let negative = try searcher.page("paginationword", queryVector: nil, limit: 10, offset: -10)
        #expect(negative.offset == 0)
        #expect(negative.hits.count == 3)

        let zero = try searcher.page("paginationword", queryVector: nil, limit: 0, offset: 0)
        #expect(zero.hits.isEmpty)
        #expect(zero.consumed == 0)
    }

    @Test func aPageAfterTheStrongBoundaryKeepsTheGlobalThreshold() throws {
        let searcher = try index(count: 6)
        let all = try searcher.search("paginationword", queryVector: nil, limit: 100)
        let page = try searcher.page("paginationword", queryVector: nil, limit: 3, offset: 3)
        #expect(page.strong == Searcher.strongCount(all))
        #expect(page.topScore == all.first?.score)
        // The page's own first score must not become the threshold for its `strong` count.
        #expect(page.hits.allSatisfy { $0.score <= (all.first?.score ?? 0) })
    }
}
