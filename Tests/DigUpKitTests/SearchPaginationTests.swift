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

    /// Images whose cosine to the query vector is known (the vector's first component, both unit vectors), for the
    /// window's cosine layers: [1, 0, …] against [cos, sin, 0, …] is `cos`.
    private func cosineIndex(_ cosines: [Float]) throws -> (Searcher, [Float]) {
        let store = try IndexStore(directory: folder)
        var query = [Float](repeating: 0, count: Vectors.dimension)
        query[0] = 1
        for (number, cosine) in cosines.enumerated() {
            let id = try store.insert(Candidate(path: "/paging/photo-\(number).jpg", kind: .image, size: 1, mtime: 1,
                                                inode: UInt64(number + 1), device: 1))
            var vector = [Float](repeating: 0, count: Vectors.dimension)
            vector[0] = cosine
            vector[1] = max(0, 1 - cosine * cosine).squareRoot()
            try store.addSegment(file: id, kind: .image, modality: .image, vector: vector)
        }
        return (try Searcher(store: store), query)
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

    @Test func imageLayersAreCutOnCosineAndPageInOrder() throws {
        let (searcher, query) = try cosineIndex([0.70, 0.66, 0.645, 0.62, 0.61, 0.55])
        let first = try searcher.page("layers", queryVector: query, kinds: [.image], limit: 3, offset: 0,
                                      cosineFloor: 0.60, byCosine: true, tierThresholds: [0.648, 0.60])
        // The 0.55 image is the hidden tail: the floor leaves it out of the ranking, but the counts still see it.
        #expect(first.total == 5)
        #expect(first.hits.map(\.path) == ["/paging/photo-0.jpg", "/paging/photo-1.jpg", "/paging/photo-2.jpg"])
        #expect(first.tierCounts?["visible"] == 2)
        #expect(first.tierCounts?["weak"] == 3)
        #expect(first.tierCounts?["hidden"] == 1)

        let next = try searcher.page("layers", queryVector: query, kinds: [.image], limit: 5, offset: 3,
                                     cosineFloor: 0.60, byCosine: true, tierThresholds: [0.648, 0.60])
        #expect(next.hits.map(\.path) == ["/paging/photo-3.jpg", "/paging/photo-4.jpg"])

        // The hidden fold is a search of its own: below the weak cutoff, in cosine order too.
        let hidden = try searcher.page("layers", queryVector: query, kinds: [.image], limit: 5, offset: 0,
                                       cosineCeiling: 0.60, byCosine: true)
        #expect(hidden.total == 1)
        #expect(hidden.hits.map(\.path) == ["/paging/photo-5.jpg"])
    }

    @Test func kindCountsCoverTheWholeRankingNotJustThePage() throws {
        let searcher = try index(count: 7)
        // A page of two must still report all seven documents: the window's chips count the query, not the page.
        let page = try searcher.page("paginationword", queryVector: nil, limit: 2, offset: 0)
        #expect(page.hits.count == 2)
        #expect(page.kindCounts[.doc] == 7)
        #expect(page.kindTotals[.doc] == 7)
        #expect(page.kindCounts.values.reduce(0, +) == page.strong)
        #expect(page.kindTotals.values.reduce(0, +) == page.total)

        let beyond = try searcher.page("paginationword", queryVector: nil, limit: 2, offset: 99)
        #expect(beyond.hits.isEmpty)
        #expect(beyond.kindCounts[.doc] == 7)
        #expect(beyond.kindTotals[.doc] == 7)
    }
}
