# DigUp engineering review

**Date:** 2026-10-24  
**Reviewed:** the Windows shell's file operations (`shell/src-tauri/src/file_ops.rs`), their index follow-up
(`Sources/digup/SearchService.swift`, `Sources/DigUpKit/IndexStore.swift`, `Sources/DigUpKit/Indexer.swift`), the
path-aware watcher (`shell/src-tauri/src/watcher.rs`, `echo.rs`), and the UI that drives them (`shell/ui/window.js`,
`window.html`). The rest of the repository was checked for build/test hygiene and for places where the new feature
contradicted existing documentation.

**Method:** read the changed code together with the engine rules it depends on (`Crawler`, `Rules`, `FolderSelection`),
ran `cargo test`, `cargo clippy --all-targets`, and `node --check` over the shell UI, and compared the behaviour with the
plan in `docs/file-operations-plan.md`. Swift tests were last run before the final docs/UI-only changes; no Swift source
changed after that run.

**Verification at the end of this pass:**

- `cargo test --locked` in `shell/src-tauri`: **36 passed**, 0 failed.
- `cargo clippy --locked --all-targets`: **no warnings**.
- `node --check` for `window.js`, `results.js`, `panel.js`, `setup.js`, `settings.js`: **ok**.
- Manual Recycle Bin restore E2E: **not run** (needs a real Explorer session).

The issues below are ordered by correctness risk, then by cost. Each has a concrete fix; none of them blocks the current
Windows beta. H1 now fails loudly instead of silently, and H2's common dot-file case is closed; the full destination
classifier is the main remaining correctness work.

---

## High

### H1. Long-path move/copy works, long-path delete still goes through the old shell API

**Status: fixed in this pass as a clear refusal.** The preflight now rejects a path at 260 UTF-16 units before calling
`SHFileOperationW`; full long-path delete is deferred to a proven `IFileOperation` spike.

`wide()` now prefixes absolute and UNC paths with `\\?\` for `MoveFileExW`, `CopyFileExW` and `CreateFileW`
(`shell/src-tauri/src/file_ops.rs:449`). The Recycle Bin path deliberately does not use that helper:

```rust
fn trash(path: &Path) -> Result<(), String> {
    let mut from: Vec<u16> = path.to_string_lossy().replace('/', "\\").encode_utf16().collect();
    from.extend([0, 0]);
    // SHFileOperationW ...
}
```

`SHFileOperationW` is the legacy shell API; Microsoft documents a `MAX_PATH` limitation and recommends
`IFileOperation` since Windows Vista. A file whose move and copy now succeed at 300 characters can therefore still fail
when it is deleted, with a shell error code instead of a useful message. The per-file report is correct in shape, but the
error is not actionable.

**Recommendation: do the small fix now; do not migrate to `IFileOperation` just for long paths.** The Recycle Bin is
the safety contract of this feature, and the evidence that `IFileOperation` removes the shell's path limit is not
solid: shell items and the Recycle Bin can still hit `MAX_PATH` on current Windows. A COM migration would add lifetime,
threading and error-model risk for a rare case without a proven benefit. Failing loudly is better than a clever
workaround that may bypass the Recycle Bin or lose the restore location.

**Fix (small, recommended now):** keep `SHFileOperationW`, but check the UTF-16 length before calling it and return a
specific message:

```rust
let units = path.to_string_lossy().encode_utf16().count();
if units >= 260 {
    return Err("its path is too long for the Recycle Bin (260 characters)".into());
}
```

That is honest but still leaves long paths undeletable.

**Fix (only after a spike proves it works):** move `trash` to `IFileOperation`:

1. `CoInitializeEx` on the worker thread (or `CoInitialize` once per operation).
2. `SHCreateItemFromParsingName` for the file.
3. `IFileOperation::SetOperationFlags(FOF_ALLOWUNDO | FOF_WANTNUKEWARNING | FOF_NOERRORUI | FOF_SILENT)`.
4. `DeleteItem`, `PerformOperations`, and read `GetAnyOperationsAborted`.

`IFileOperation` takes shell items rather than a `MAX_PATH` string and is the API Explorer itself uses. Add a test that
creates a >260-character path, moves it, copies it and deletes it, then restores it manually once. The current long-path
test (`a_long_path_moves_and_copies`) covers only the first two steps.

**Why this is not the default:** the COM migration changes the error model for every delete and needs a real Recycle Bin
restore check on the oldest supported Windows. Spike it behind a test harness, prove that a 300-character path is
recycled **and restored to its original path**, then decide. Until then the preflight message is the better product.

---

### H2. A moved dot-file stays in the index although the crawler never reads it

**Status: fixed in this pass, minimum form.** `isCrawled` now rejects a dot-prefixed file name before checking the
folder; the full classifier (Windows hidden attribute, excluded types, datasets) remains in “Remaining concerns”.

`SearchService.isCrawled` checks the destination **folder**, with this comment:

```swift
// The folder, not the file: a file named `.photo.jpg` is still one the crawl classifies.
let target = FolderSelection.realPath(FolderSelection.parent(path))
```

That assumption is wrong. On Windows the custom `DirectoryWalk` leaves out “hidden files (the attribute, or a name
starting with a dot, as on a Mac)” (`Sources/DigUpKit/Platform/Portable/Files+Portable.swift:174-176`). On macOS the
crawler uses `FileManager.enumerator(..., options: [.skipsHiddenFiles, ...])`
(`Sources/DigUpKit/Crawler.swift:101`). A file moved to `Pictures/.photo.jpg` is therefore re-pathed in the index, but
the next crawl skips it. Until a full sync it stays searchable at a path the indexer considers invisible.

The same gap covers the Windows hidden attribute (a normal name with `FILE_ATTRIBUTE_HIDDEN`) and, less directly,
destination-level exclusions that `Rules.isWalked` does not model (for example a file whose extension was excluded
after the search, or a folder that becomes a dataset because of the moved file).

**Fix (minimum):** reject dot-prefixed destination names before asking `isCrawled`:

```swift
private func isCrawled(_ path: String) -> Bool {
    let name = (path as NSString).lastPathComponent
    guard !name.hasPrefix(".") else { return false }   // the walk skips hidden names
    // ... existing folder check ...
}
```

Add a unit test around the decision. `SearchService` lives in the `digup` executable target, so either extract this
predicate into `DigUpKit` (the clean fix) or cover it through the `file_op` protocol in an integration test.

**Fix (complete):** make the destination file go through the same `Rules.classify` path as the crawler, including the
Windows hidden attribute and the dataset rules. That is more work because `SearchService` currently has only strings
(`roots`, `skipped`) and not `IndexOptions`.

---

## Medium

### M1. A 5,000-file batch performs 5,000 synchronous index round trips

The file phase is batched, but the index phase is not. For every moved or deleted file, `operate` calls
`index_move`/`index_delete` (`shell/src-tauri/src/file_ops.rs:200-245`), and each call does:

```rust
let reply = tauri::async_runtime::block_on(shell.search.ask(request))
```

with one `file_op` item. The helper protocol already accepts an array (`SearchService` loops over `items` and returns
one result per item), so the round trips are avoidable. On a large selection this adds a request/response and a
`std::fs::metadata` + `CreateFileW` identity probe per file, on top of the file-system work. The plan’s “5,000 moves”
case is exactly where this shows up.

**Fix:** collect moves and deletes while the files are being processed, then send at most two batched requests at the
end (one for moves, one for deletes) with `reload: false`, followed by the existing one `reload`. The per-item results
already come back in `reply["results"]`; `ask` must stop checking only `results[0]` and return the array so the report
can attach `index_error` to the right entries. Keep the per-file echo notes exactly where they are: they must be
written before the index changes.

**Trade-off:** a batch waits to the end before the in-memory vectors follow the new paths. Rows read paths from the
database, so search results remain correct; only the vector cache is stale longer. That is already the intended
design of `reload:false`.

---

### M2. A failed cleanup after a partial copy is swallowed

**Status: fixed in this pass.** The returned error now includes the cleanup failure.

`copy_to_free` removes a partial destination after a failed `CopyFileExW`:

```rust
Err(error) => {
    let _ = std::fs::remove_file(&target);
    return Err(why(&error));
}
```

If the remove fails (a scanner holds the file, permissions, a cloud filter), the user gets the copy error but the
half-written file remains in the destination folder. Nothing in the report names it, so the next attempt can create
`photo (2).jpg` beside an unexplained `photo.jpg`.

**Fix:** include the cleanup result in the error, and add the destination to the report item:

```rust
Err(error) => {
    let cleanup = std::fs::remove_file(&target);
    let message = match cleanup {
        Ok(()) => why(&error),
        Err(cleanup) => format!("{}; the partial copy couldn't be removed: {cleanup}", why(&error)),
    };
    return Err(message);
}
```

The caller already puts the error text into `report.items`; no schema change is needed. A test can lock the
destination after a forced failure, but the common path is enough to lock the message down.

---

### M3. `Show all N` builds the whole list in one synchronous pass

**Status: fixed in this pass.** The list now grows in 500-line chunks and keeps the focus on the button between clicks.

`reportList` creates the first eight rows, then on the click does:

```js
more.onclick = () => {
  fill(items.length);
  ...
};
```

For a 5,000-file operation, `items.length` lines are created in one task. Each line is several DOM nodes, so the
WebView can freeze for a noticeable moment exactly when the user asks for the details. The plan explicitly calls out
5,000 moved files.

**Fix:** render in chunks, or keep the button and append the next 500 on each click:

```js
const CHUNK = 500;
let shown = REPORT_SHOWN;
const more = element("button", "report-more", `Show all ${items.length.toLocaleString()}`);
const fill = (count) => {
  list.replaceChildren();
  for (const item of items.slice(0, count)) list.append(line(item));
};
const showMore = () => {
  shown = Math.min(items.length, shown + CHUNK);
  fill(shown);
  if (shown === items.length) more.remove();
  else more.textContent = `Show ${Math.min(CHUNK, items.length - shown).toLocaleString()} more`;
  $report.classList.add("expanded");
};
```

This keeps the first click cheap and still makes every name reachable. A virtualized list is unnecessary here.

---

### M4. `Copy Paths` silently does nothing when the clipboard is unavailable

**Status: fixed in this pass.** Success goes to the status note, failure to the normal error line.

`copyPaths` ends with:

```js
navigator.clipboard.writeText(paths.join("\n")).catch(() => {});
```

If WebView2 denies the clipboard write (focus change, permission policy, a locked clipboard), the user gets no
feedback. The context menu action and the batch-bar button look successful.

**Fix:** reuse the status line the other copy action uses:

```js
navigator.clipboard.writeText(paths.join("\n"))
  .then(() => setNote(paths.length === 1 ? "Copied the path" : `Copied ${paths.length.toLocaleString()} paths`))
  .catch((error) => failed(`The paths couldn't be copied: ${error}`));
```

`setNote` already clears itself after ten seconds. Add a DOM-level check if the UI tests grow.

---

### M5. The watcher treats every `.ts` change as media, even in the main index

`change_may_matter` keeps `.ts`/`.mts`/`.m2ts` because a TypeScript file and an MPEG transport stream share the
extension (`shell/src-tauri/src/watcher.rs:152`). In the main (non-code) watcher a TypeScript save therefore asks for a
sync even though the crawl will not read it; in a code folder the code watcher handles it separately. This is a
deliberate correctness-over-work trade-off, not a broken result.

**Fix if it becomes noisy:** pass the watcher’s purpose (`code` vs main) or probe the first bytes with the same MPEG-TS
check the engine uses (`Rules.isMPEGTransportStream`). Until then, leave it and keep the comment.

---

## Low

### L1. The mtime guard assumes sub-second timestamps

`check_source` rejects a file when `(now - mtime).abs() > 1.0` (`file_ops.rs:266-268`). NTFS and APFS store fine
timestamps, but FAT/exFAT (a USB stick, an SD card) has two-second write granularity. A file that changed on such a
volume can be skipped even though its content is still what the search showed. The cost is a false “it changed since
the search” message; the user can retry or copy manually. This is worth a comment and a manual E2E on a FAT stick, not
a code change without a way to detect the volume.

### L2. `free_path`’s last resort can itself be taken

After `(2)` … `(9999)`, `free_path` returns `stem (copied).ext` without checking that it is free. Both `MoveFileExW`
(without replace) and `CopyFileExW` (with `COPY_FILE_FAIL_IF_EXISTS`) still refuse to overwrite, so this fails safely.
The only cost is an unhelpful “there's already a file with that name” after 9,998 collisions. Leave it.

### L3. `refresh_counts` is fire-and-forget

After a move/delete, `main.rs` spawns `refresh_counts` and does not await it. The report does not depend on it, and a
failed refresh only leaves the status bar’s counts stale until the next status event. This is intentional; document it
so a later reader does not “fix” it by awaiting a possibly stopped helper.

---

## Fixed during this pass

These were found in the same review and are already in `main`, so they are not open findings:

| Issue | Fix | Commit |
|---|---|---|
| `MoveFileExW`/`CopyFileExW` failed at paths over 260 characters | `wide()` adds `\\?\` / `\\?\UNC\`; `SHFileOperationW` stays unprefixed | `787ee43` |
| A dehydrated OneDrive/File Provider file would be downloaded by move/copy | preflight reads `FILE_ATTRIBUTE_OFFLINE`/`RECALL_ON_*` and skips with a clear reason | `31c563e` |
| Read-only and busy files had no regression coverage | move/copy read-only; a file opened with no sharing stays put and is reported | `5c19d47` |
| Unicode names (`é`, `日`, `ı`, `ö`) had no regression coverage | move/copy test | `a77927a` |
| Dead full-code sync helper and outdated plan claims | removed; plan updated | `787ee43`, `1e1df9b` |
| Shell clippy warnings | test-only imports moved; `then_some`, `is_multiple_of`, `is_none_or`, `next_back` | `cd85c2c` |
| README said DigUp “never changes, moves” files | now documents Move/Copy/Delete and the Recycle Bin | `2ebc94d` |
| A moved dot-file stayed indexed although the crawl skips hidden names | `isCrawled` rejects dot-prefixed names | this pass |
| A failed partial-copy cleanup was swallowed | the error now names the file that couldn't be removed | this pass |
| `Copy Paths` failed silently | success and failure both reach the status bar | this pass |
| `Show all N` built thousands of rows at once | the report appends 500 at a time | this pass |
| Delete over 260 characters reached the shell and failed obscurely | preflight refuses it with a clear message | this pass |

The current feature is in a good state: the core paths are tested, the index follows without re-embedding, and the
Windows-specific behaviour (wide paths, cloud placeholders, echo suppression) has direct tests. The remaining work is
edge handling, not a rewrite.

---

## Remaining concerns that are not safe to fix automatically

1. **Recycle Bin restore E2E.** Needs Explorer, a real user confirmation and a manual restore. The index behaviour after
   restore is expected to be a fresh embed (the delete removed the row); verify it once.
2. **A screen-reader pass.** The markup has `role="listbox"`/`option`, `aria-selected`, `aria-keyshortcuts`, dialog
   focus and Tab trapping, but the focused option is not exposed as `aria-activedescendant` while the keyboard stays in
   the query field. Deciding whether to move focus into the grid is a UX decision, not a mechanical fix.
3. **`IFileOperation` spike, not migration yet.** It is the right API in general, but it is not proven to fix the
   Recycle Bin's long-path limit; test delete **and restore** on Windows 10/11 before replacing `SHFileOperationW`.
4. **Destination-level rules.** Dataset detection and Windows hidden attributes need the crawler’s full classifier in
   `SearchService`; until then H2’s minimum fix covers the common dot-file case.
5. **Large-batch index batching.** M1 changes error attribution in the report and needs a test that mixes successful,
   failed and skipped files in one batch.

## Assumptions

- The first platform is Windows 10/11 on NTFS; FAT/exFAT is an edge case for L1.
- File operations are offered only for media results in the big window, as the plan says; documents and code are out of
  scope.
- `SHFileOperationW` is still acceptable for ordinary Recycle Bin deletes; the long-path case is the exception.
- The search helper and index helper are separate processes and may be reading the same file while `file_op` runs; the
  `stillIndexed` guard in `Indexer.commit` is the intended protection and was not changed.
- No telemetry or network calls are introduced by any of these fixes.
