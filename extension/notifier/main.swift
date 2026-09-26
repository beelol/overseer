// Overseer Notifier (AC-52): posts Overseer's macOS notifications under Overseer's own name
// and icon, and opens VS Code at the Overseer view when a notification is clicked.
//
//   notifier --title T --body B [--open URL]   post; exit 0 posted, 3 denied, 4 error, 5 no answer yet
//   notifier --status                           print notDetermined|denied|authorized|provisional
//   (no arguments)                              launched by macOS for a click: open the URL, then quit
//
// Built by extension/scripts/package.js into bin/Overseer Notifier.app (LSUIElement, ad-hoc signed).
import AppKit
import UserNotifications

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
            DispatchQueue.main.asyncAfter(deadline: .now() + 20) { FileHandle.standardError.write("no answer to the permission prompt yet\n".data(using: .utf8)!); exit(5) }
            center.requestAuthorization(options: [.alert, .sound]) { granted, error in
                guard granted else {
                    FileHandle.standardError.write("notifications denied\(error.map { ": \($0.localizedDescription)" } ?? "")\n".data(using: .utf8)!)
                    exit(3)
                }
                let content = UNMutableNotificationContent()
                content.title = title
                content.body = body
                if let open { content.userInfo = ["open": open] }
                let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
                self.center.add(request) { error in
                    if let error {
                        FileHandle.standardError.write("could not post: \(error.localizedDescription)\n".data(using: .utf8)!)
                        exit(4)
                    }
                    print("posted")
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { exit(0) }
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
