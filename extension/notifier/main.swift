// Overseer Notifier (AC-52): posts Overseer's macOS notifications under Overseer's own name
// and icon, and opens VS Code at the Overseer view when a notification is clicked.
//
//   notifier --title T --body B [--open URL] [--result FILE]
//                                               post; exit 0 posted, 3 denied, 4 error, 5 no answer yet.
//                                               The daemon launches it through LaunchServices (`open -W`),
//                                               which hides the exit code, so the outcome is also written
//                                               to FILE as "<code> <message>".
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
    case post(title: String, body: String, open: String?)
    case status
    case click
}

func parse(_ args: [String]) -> Mode {
    if args.contains("--status") { return .status }
    func value(_ flag: String) -> String? {
        guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
        return args[i + 1]
    }
    resultFile = value("--result")
    if let title = value("--title") { return .post(title: title, body: value("--body") ?? "", open: value("--open")) }
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
        case let .post(title, body, open):
            // Never wait forever for the first-time permission prompt; the daemon falls back.
            DispatchQueue.main.asyncAfter(deadline: .now() + 20) { finish(5, "no answer to the permission prompt yet") }
            center.requestAuthorization(options: [.alert, .sound]) { granted, error in
                guard granted else { finish(3, "notifications denied\(error.map { ": \($0.localizedDescription)" } ?? "")") }
                let content = UNMutableNotificationContent()
                content.title = title
                content.body = body
                if let open { content.userInfo = ["open": open] }
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
        if let s = response.notification.request.content.userInfo["open"] as? String, let url = URL(string: s), ["vscode", "vscode-insiders"].contains(url.scheme ?? "") {
            NSWorkspace.shared.open(url)
        }
        done()
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { exit(0) }
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler done: @escaping (UNNotificationPresentationOptions) -> Void) {
        done([.banner, .list, .sound])
    }
}

let app = NSApplication.shared
let delegate = Notifier(mode: parse(Array(CommandLine.arguments.dropFirst())))
app.delegate = delegate
app.setActivationPolicy(.accessory)
app.run()
