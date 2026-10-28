/// The shell and its helpers speak one JSON-line protocol; this number moves whenever a release changes what the
/// helpers accept or reply. The shell pins its own copy (`shell/src-tauri/src/main.rs`, `HELPER_PROTOCOL`) and passes
/// `--protocol <n>` to every spawn, so a shell-only update can't pair with a helper from another generation: the
/// helper refuses at once, and the message says to take the full package. Manual CLI runs pass nothing and always
/// match their own build.
enum HelperProtocol {
    static let version = 1
}
