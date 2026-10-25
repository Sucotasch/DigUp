import Foundation
import Testing
@testable import DigUpKit

@Suite final class ModelFilesTests {
    let folder: URL = {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("DigUp-model-files-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }()

    deinit { try? FileManager.default.removeItem(at: folder) }

    /// A sparse file: its logical size without writing (or allocating) 865 MB.
    func sparse(_ name: String, bytes: Int64) throws -> URL {
        let url = folder.appendingPathComponent(name)
        _ = FileManager.default.createFile(atPath: url.path, contents: nil)
        try FileHandle(forWritingTo: url).truncate(atOffset: UInt64(bytes))
        return url
    }

    @Test func aFileWithTheWrongSizeIsStillMissing() throws {
        _ = try sparse(ModelFiles.text.name, bytes: 10)
        #expect(ModelFiles.missing(in: folder) == [ModelFiles.text, ModelFiles.media])
        #expect(!ModelFiles.complete(in: folder))
        let text = folder.appendingPathComponent(ModelFiles.text.name)
        try FileHandle(forWritingTo: text).truncate(atOffset: UInt64(ModelFiles.text.bytes))
        #expect(ModelFiles.missing(in: folder) == [ModelFiles.media])
        _ = try sparse(ModelFiles.media.name, bytes: ModelFiles.media.bytes)
        #expect(ModelFiles.missing(in: folder).isEmpty)
        #expect(ModelFiles.complete(in: folder))
    }

    @Test func verifyNamesTheProblem() throws {
        let file = ModelFiles.File(name: "small.bin", bytes: 3, sha256:
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")   // SHA-256 of "abc"
        #expect(ModelFiles.check(file, in: folder, hash: true)
            == ModelFiles.Check(file: file, present: false, size: 0, ok: false, problem: "missing"))
        try Data("ab".utf8).write(to: folder.appendingPathComponent(file.name))
        #expect(ModelFiles.check(file, in: folder, hash: true)
            == ModelFiles.Check(file: file, present: true, size: 2, ok: false, problem: "wrong size"))
        try Data("abd".utf8).write(to: folder.appendingPathComponent(file.name))
        #expect(ModelFiles.check(file, in: folder, hash: true)
            == ModelFiles.Check(file: file, present: true, size: 3, ok: false, problem: "checksum mismatch"))
        try Data("abc".utf8).write(to: folder.appendingPathComponent(file.name))
        #expect(ModelFiles.check(file, in: folder, hash: true).ok)
        // Sizes alone are enough to say a download is complete, as every file gets its name only after its hash was
        // checked.
        #expect(ModelFiles.check(file, in: folder).ok)
    }

    @Test func verifyReportsEveryModelFile() {
        let checks = ModelFiles.verify(in: folder)
        #expect(checks.map(\.file) == ModelFiles.all)
        #expect(checks.allSatisfy { !$0.ok && $0.problem == "missing" })
    }

    @Test func freeSpaceIsKnownForAFolderThatExists() {
        #expect((ModelFiles.freeSpace(at: folder) ?? 0) > 0)
    }
}
