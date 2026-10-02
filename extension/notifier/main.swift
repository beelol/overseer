// Overseer Notifier (AC-52): posts Overseer's macOS notifications under Overseer's own name
// and icon, and opens VS Code at the Overseer view when a notification is clicked.
//
//   notifier --title T --body B [--open URL] [--thread ID] [--tui CMD] [--result FILE]
//                                               post; exit 0 posted, 3 denied, 4 error, 5 no answer yet.
//                                               The daemon launches it through LaunchServices (`open -W`),
//                                               which hides the exit code, so the outcome is also written
//                                               to FILE as "<code> <message>". --thread groups the
//                                               notification with others of the same agent (AC-240).
//                                               --tui is the shell command that opens the TUI on that
//                                               agent: a click with VS Code closed runs it in Terminal.
//   notifier --simulate-click --open URL [--tui CMD] --vscode running|closed
//                                               what a click would do, printed, nothing opened (tests)
//   notifier --status                           print notDetermined|denied|authorized|provisional
//   (no arguments)                              launched by macOS for a click: open the URL, then quit
//
// Built by extension/scripts/package.js into bin/Overseer Notifier.app (LSUIElement, ad-hoc signed).
import AppKit
import UserNotifications

/// Where to report the outcome when launched with `open` (set from --result).
var resultFile: String?

/// Records the outcome for the daemon, then exits with the same code.
func finish(_ code: Int32, _ message: String) -> Never {
    if let path = resultFile { try? "\(code) \(message)\n".write(toFile: path, atomically: true, encoding: .utf8) }
    if code == 0 { print(message) } else { FileHandle.standardError.write("\(message)\n".data(using: .utf8)!) }
    exit(code)
}

enum Mode {
    case post(title: String, body: String, open: String?, thread: String?, tui: String?)
    case status
    case click
    case simulate(open: String?, tui: String?, vscodeRunning: Bool)
}

func parse(_ args: [String]) -> Mode {
    if args.contains("--status") { return .status }
    func value(_ flag: String) -> String? {
        guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
        return args[i + 1]
    }
    resultFile = value("--result")
    if args.contains("--simulate-click") { return .simulate(open: value("--open"), tui: value("--tui"), vscodeRunning: value("--vscode") != "closed") }
    if let title = value("--title") { return .post(title: title, body: value("--body") ?? "", open: value("--open"), thread: value("--thread"), tui: value("--tui")) }
    return .click
}

final class Notifier: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    let mode: Mode
    let center = UNUserNotificationCenter.current()
    init(mode: Mode) { self.mode = mode }

    func applicationDidFinishLaunching(_ note: Notification) {
        center.delegate = self
        switch mode {
        case .status:
            center.getNotificationSettings { settings in
                let s: String
                switch settings.authorizationStatus {
                case .authorized: s = "authorized"
                case .denied: s = "denied"
                case .provisional: s = "provisional"
                case .ephemeral: s = "ephemeral"
                default: s = "notDetermined"
                }
                print(s)
                exit(0)
            }
        case .simulate:
            exit(0) // handled before the app starts (simulate(), below)
        case let .post(title, body, open, thread, tui):
            // Never wait forever for the first-time permission prompt; the daemon falls back.
            DispatchQueue.main.asyncAfter(deadline: .now() + 20) { finish(5, "no answer to the permission prompt yet") }
            center.requestAuthorization(options: [.alert, .sound]) { granted, error in
                guard granted else { finish(3, "notifications denied\(error.map { ": \($0.localizedDescription)" } ?? "")") }
                let content = UNMutableNotificationContent()
                content.title = title
                content.body = body
                var info: [String: String] = [:]
                if let open { info["open"] = open }
                if let tui { info["tui"] = tui }
                content.userInfo = info
                if let thread { content.threadIdentifier = thread }
                let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
                self.center.add(request) { error in
                    if let error { finish(4, "could not post: \(error.localizedDescription)") }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { finish(0, "posted") }
                }
            }
        case .click:
            // Launched by macOS for a notification click; the response arrives via the delegate.
            DispatchQueue.main.asyncAfter(deadline: .now() + 10) { exit(0) }
        }
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler done: @escaping () -> Void) {
        let info = response.notification.request.content.userInfo
        switch route(info, vscodeRunning: vscodeRunning(for: info["open"] as? String)) {
        case let .url(url): NSWorkspace.shared.open(url)
        case let .terminal(file): NSWorkspace.shared.open(file)
        case .nothing: break
        }
        done()
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { exit(0) }
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler done: @escaping (UNNotificationPresentationOptions) -> Void) {
        done([.banner, .list, .sound])
    }
}

/// Where a click goes (AC-240): that agent in VS Code while VS Code runs; with VS Code closed, the
/// TUI on that agent in Terminal (a `.command` file, which Terminal opens and runs; it removes
/// itself), else VS Code's URL, which starts VS Code.
enum Route { case url(URL), terminal(URL), nothing }

func route(_ info: [AnyHashable: Any], vscodeRunning: Bool) -> Route {
    let url = (info["open"] as? String).flatMap(URL.init(string:)).flatMap { ["vscode", "vscode-insiders"].contains($0.scheme ?? "") ? $0 : nil }
    if !vscodeRunning, let tui = info["tui"] as? String, !tui.isEmpty {
        let file = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("overseer-tui-\(UUID().uuidString.prefix(8)).command")
        let script = "#!/bin/sh\n# Overseer: the agent from the notification, in the terminal UI (VS Code is closed).\nrm -f \"$0\"\nexec \(tui)\n"
        if (try? script.write(to: file, atomically: true, encoding: .utf8)) != nil,
           (try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: file.path)) != nil {
            return .terminal(file)
        }
    }
    return url.map { .url($0) } ?? .nothing
}

/// Whether the VS Code the URL is for is running (Insiders for vscode-insiders:).
func vscodeRunning(for open: String?) -> Bool {
    let ids = (open ?? "").hasPrefix("vscode-insiders:") ? ["com.microsoft.VSCodeInsiders"] : ["com.microsoft.VSCode"]
    return ids.contains { !NSRunningApplication.runningApplications(withBundleIdentifier: $0).isEmpty }
}

/// `--simulate-click`: prints where a click would go and writes the Terminal file, opening nothing.
/// Runs before the app (no notification center: it works outside the app bundle, in tests).
func simulate(open: String?, tui: String?, vscodeRunning running: Bool) -> Never {
    var info: [AnyHashable: Any] = [:]
    if let open { info["open"] = open }
    if let tui { info["tui"] = tui }
    switch route(info, vscodeRunning: running) {
    case let .url(url): print("open \(url.absoluteString)")
    case let .terminal(file): print("terminal \(file.path)")
    case .nothing: print("nothing")
    }
    exit(0)
}

let mode = parse(Array(CommandLine.arguments.dropFirst()))
if case let .simulate(open, tui, running) = mode { simulate(open: open, tui: tui, vscodeRunning: running) }
let app = NSApplication.shared
let delegate = Notifier(mode: mode)
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
