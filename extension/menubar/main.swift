// Overseer Menu (AC-262): Overseer in the Mac's menu bar. The owner's silhouette as a template
// image, a dot while an agent needs the owner, and a menu with every Needs-you request (Allow once
// with Always allow under its arrow, Deny), a one-line summary, one submenu per repository (the 8
// most recent agents, then "Show all in Overseer…"), Voice Mode and mute, Open Overseer, Talk to
// Overseer and Quit. It is only a display: everything comes from the daemon over its socket
// (menubar.snapshot, events.subscribe) and every answer goes back to it (run.permission,
// voice.set). Choosing an agent opens it in VS Code (vscode://beelol.overseer/open-agent?run=…).
//
//   overseer-menu                          the item (started at login; see --login-item)
//   overseer-menu --config FILE            read FILE instead of menubar.json beside the app
//   overseer-menu --login-item on|off|status
//                                          register with macOS as a login item (scripts/deploy only)
//   overseer-menu --capture PREFIX [--appearance light|dark] [--submenu REPO] [--when TEXT]
//                                          open the menu, write PREFIX.png (the item on a menu bar,
//                                          the menu and the open submenu) and quit: AC-262's evidence
//   overseer-menu --press allow|always|deny [--index N]
//                                          press that control on the N-th request in the open menu,
//                                          print the daemon's answer and quit
//   overseer-menu --choose REPO --index N  choose the N-th agent of REPO's submenu and quit
//   overseer-menu --choose-item TITLE      choose the menu item titled TITLE (Start Overseer) and quit
//
// menubar.json: {"socket", "daemon", "instance", "start": [argv], "code": "…/bin/code",
// "code_args": [...]}; every key optional. Written by scripts/deploy (the installed Overseer) and
// scripts/dev (a dev daemon, whose item carries a "!" and its name).
//
// Built by extension/menubar/build.js into bin/Overseer Menu.app (LSUIElement, ad-hoc signed).
import AppKit
import ServiceManagement

// MARK: - Arguments and configuration

let args = CommandLine.arguments
func arg(_ flag: String) -> String? {
    guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
    return args[i + 1]
}
let debugging = ProcessInfo.processInfo.environment["OVERSEER_MENU_DEBUG"] != nil
func trace(_ s: @autoclosure () -> String) { if debugging { FileHandle.standardError.write(("menu: " + s() + "\n").data(using: .utf8)!) } }
func say(_ s: String) { FileHandle.standardOutput.write((s + "\n").data(using: .utf8)!) }
func fail(_ message: String, _ code: Int32 = 1) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(code)
}

struct Config {
    var socket: String?
    var daemon: String?
    var instance: String?
    var start: [String] = []
    var code: String?
    var codeArgs: [String] = []

    static func load() -> Config {
        let beside = Bundle.main.bundleURL.deletingLastPathComponent().appendingPathComponent("menubar.json").path
        let path = arg("--config") ?? ProcessInfo.processInfo.environment["OVERSEER_MENU_CONFIG"] ?? beside
        var c = Config()
        if let data = FileManager.default.contents(atPath: path),
           let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] {
            c.socket = j["socket"] as? String
            c.daemon = j["daemon"] as? String
            c.instance = j["instance"] as? String
            c.start = j["start"] as? [String] ?? []
            c.code = j["code"] as? String
            c.codeArgs = j["code_args"] as? [String] ?? []
        }
        // Next to the daemon in the extension's bin/ when nothing says otherwise.
        if c.daemon == nil {
            let bin = Bundle.main.bundleURL.deletingLastPathComponent()
            #if arch(arm64)
            let name = "overseerd-darwin-arm64"
            #else
            let name = "overseerd-darwin-x64"
            #endif
            let p = bin.appendingPathComponent(name).path
            if FileManager.default.isExecutableFile(atPath: p) { c.daemon = p }
        }
        if c.start.isEmpty, let d = c.daemon { c.start = [d, "serve"] }
        return c
    }

    var dev: Bool { instance?.hasPrefix("dev-") ?? false }

    /// The daemon's socket: the configured one, else what the daemon binary says.
    func socketPath() -> String? {
        if let s = socket { return s }
        guard let d = daemon else { return nil }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: d)
        p.arguments = ["socket-path"]
        var env = ProcessInfo.processInfo.environment
        for k in env.keys where k.hasPrefix("OVERSEER_") { env.removeValue(forKey: k) }
        p.environment = env
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        do { try p.run() } catch { return nil }
        p.waitUntilExit()
        let s = String(data: out.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines)
        return (s?.isEmpty ?? true) ? nil : s
    }
}

// MARK: - The daemon's socket (JSON lines)

final class Link {
    let fd: Int32
    var onMessage: (([String: Any]) -> Void)?
    var onClose: (() -> Void)?
    private var closed = false

    init?(path: String) {
        fd = socket(AF_UNIX, SOCK_STREAM, 0)
        if fd < 0 { return nil }
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        guard bytes.count < MemoryLayout.size(ofValue: addr.sun_path) else { Darwin.close(fd); return nil }
        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            for (i, b) in bytes.enumerated() { raw[i] = b }
            raw[bytes.count] = 0
        }
        let rc = withUnsafePointer(to: &addr) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) } }
        if rc != 0 { Darwin.close(fd); return nil }
        var one: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &one, socklen_t(MemoryLayout<Int32>.size))
        let t = Thread { [weak self] in self?.readLoop() }
        t.start()
    }

    private func readLoop() {
        var buf = Data()
        var chunk = [UInt8](repeating: 0, count: 65536)
        while true {
            let n = read(fd, &chunk, chunk.count)
            if n <= 0 { break }
            buf.append(chunk, count: n)
            while let nl = buf.firstIndex(of: 10) {
                let line = buf.subdata(in: buf.startIndex..<nl)
                buf.removeSubrange(buf.startIndex...nl)
                if let obj = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any] {
                    DispatchQueue.main.async { self.onMessage?(obj) }
                }
            }
        }
        DispatchQueue.main.async { self.close() }
    }

    func send(_ obj: [String: Any]) {
        guard !closed, var data = try? JSONSerialization.data(withJSONObject: obj) else { return }
        data.append(10)
        data.withUnsafeBytes { raw in
            var off = 0
            while off < raw.count {
                let n = write(fd, raw.baseAddress! + off, raw.count - off)
                if n <= 0 { break }
                off += n
            }
        }
    }

    func close() {
        if closed { return }
        closed = true
        shutdown(fd, SHUT_RDWR)
        Darwin.close(fd)
        onClose?()
    }
}

// MARK: - What the menu shows (menubar.snapshot)

struct Waiting {
    let runId: String, requestId: String?, answerable: Bool, question: String, always: String?
    let harness: String, repo: String, title: String, account: String, ago: String
}
struct AgentRow { let runId: String, title: String, kind: String, status: String, account: String }
struct Repo { let name: String, root: String, count: Int, waiting: Bool, agents: [AgentRow] }
struct Snapshot {
    var summary = "", waiting: [Waiting] = [], repos: [Repo] = []
    var voiceEnabled = false, voiceMuted = false, voiceAvailable = false
    var cursor: Int = 0, instance: String?

    init() {}
    init(_ j: [String: Any]) {
        summary = j["summary"] as? String ?? ""
        cursor = j["cursor"] as? Int ?? 0
        instance = j["instance"] as? String
        let v = j["voice"] as? [String: Any] ?? [:]
        voiceEnabled = v["enabled"] as? Bool ?? false
        voiceMuted = v["muted"] as? Bool ?? false
        voiceAvailable = v["available"] as? Bool ?? false
        waiting = (j["waiting"] as? [[String: Any]] ?? []).map { w in
            Waiting(runId: w["run_id"] as? String ?? "", requestId: w["request_id"] as? String, answerable: w["answerable"] as? Bool ?? false,
                    question: w["question"] as? String ?? "", always: w["always"] as? String, harness: w["harness"] as? String ?? "",
                    repo: w["repo"] as? String ?? "", title: w["title"] as? String ?? "", account: w["account"] as? String ?? "", ago: w["ago"] as? String ?? "")
        }
        repos = (j["repos"] as? [[String: Any]] ?? []).map { r in
            Repo(name: r["name"] as? String ?? "", root: r["root"] as? String ?? "", count: r["count"] as? Int ?? 0, waiting: r["waiting"] as? Bool ?? false,
                 agents: (r["agents"] as? [[String: Any]] ?? []).map { a in
                     AgentRow(runId: a["run_id"] as? String ?? "", title: a["title"] as? String ?? "", kind: a["kind"] as? String ?? "idle",
                              status: a["status"] as? String ?? "", account: a["account"] as? String ?? "")
                 })
        }
    }
}

// MARK: - Drawing

let violet = NSColor(name: nil) { a in
    a.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? NSColor(srgbRed: 0xA4 / 255.0, green: 0x8B / 255.0, blue: 1, alpha: 1)
                                                       : NSColor(srgbRed: 0x6B / 255.0, green: 0x47 / 255.0, blue: 0xE0 / 255.0, alpha: 1)
}

/// The silhouette as a template image, with corners cut for the dot and a dev daemon's "!".
func markImage(dot: Bool, dev: Bool) -> NSImage {
    let size = NSSize(width: 18, height: 18)
    let source = Bundle.main.image(forResource: "StatusIcon")
    let img = NSImage(size: size, flipped: false) { rect in
        guard let ctx = NSGraphicsContext.current?.cgContext else { return false }
        source?.draw(in: rect)
        ctx.setBlendMode(.clear)
        if dot { ctx.fillEllipse(in: CGRect(x: 15.3 - 5.3, y: 18 - 3.3 - 5.3, width: 10.6, height: 10.6)) }
        if dev { ctx.fillEllipse(in: CGRect(x: 15 - 5.4, y: 18 - 14.2 - 5.4, width: 10.8, height: 10.8)) }
        ctx.setBlendMode(.normal)
        if dev {
            let bang = NSAttributedString(string: "!", attributes: [.font: NSFont.systemFont(ofSize: 10.5, weight: .heavy), .foregroundColor: NSColor.black])
            let s = bang.size()
            bang.draw(at: NSPoint(x: 15 - s.width / 2, y: -0.6))
        }
        return true
    }
    img.isTemplate = true
    return img
}

/// The violet dot beside the template image (a template cannot carry a colour of its own).
final class DotView: NSView {
    override func draw(_ dirtyRect: NSRect) {
        violet.setFill()
        NSBezierPath(ovalIn: bounds).fill()
    }
}

func circleImage(_ color: NSColor, hollow: Bool = false) -> NSImage {
    let img = NSImage(size: NSSize(width: 16, height: 16), flipped: false) { _ in
        let r = NSRect(x: 3.8, y: 3.8, width: 8.4, height: 8.4)
        if hollow {
            let p = NSBezierPath(ovalIn: r.insetBy(dx: 0.4, dy: 0.4)); p.lineWidth = 1.2
            NSColor.secondaryLabelColor.setStroke(); p.stroke()
        } else { color.setFill(); NSBezierPath(ovalIn: r).fill() }
        return true
    }
    return img
}
func statusImage(_ kind: String) -> NSImage {
    switch kind {
    case "working": return circleImage(.systemGreen)
    case "needs": return circleImage(.systemOrange)
    case "review": return circleImage(.systemBlue)
    default: return circleImage(.clear, hollow: true)
    }
}
func symbol(_ name: String) -> NSImage? {
    let img = NSImage(systemSymbolName: name, accessibilityDescription: nil)
    img?.isTemplate = true
    return img
}
/// A folder with an orange pip when one of its agents needs the owner.
func folderImage(waiting: Bool) -> NSImage? {
    guard waiting, let folder = NSImage(systemSymbolName: "folder", accessibilityDescription: nil) else { return symbol("folder") }
    let tinted = folder.withSymbolConfiguration(NSImage.SymbolConfiguration(paletteColors: [.labelColor])) ?? folder
    return NSImage(size: NSSize(width: 18, height: 16), flipped: false) { _ in
        tinted.draw(in: NSRect(x: 0, y: 1, width: 16, height: 14))
        NSColor.systemOrange.setFill()
        NSBezierPath(ovalIn: NSRect(x: 11, y: 9.5, width: 7, height: 7)).fill()
        return true
    }
}
func warnImage() -> NSImage? {
    NSImage(systemSymbolName: "exclamationmark.circle.fill", accessibilityDescription: "Needs you")?
        .withSymbolConfiguration(NSImage.SymbolConfiguration(paletteColors: [.white, .systemOrange]))
}

func label(_ text: String, size: CGFloat, weight: NSFont.Weight = .regular, secondary: Bool = false) -> NSTextField {
    let f = NSTextField(labelWithString: text)
    f.font = NSFont.systemFont(ofSize: size, weight: weight)
    f.textColor = secondary ? .secondaryLabelColor : .labelColor
    f.lineBreakMode = .byTruncatingTail
    return f
}

// MARK: - A request, answerable in the menu

final class AskView: NSView {
    let item: Waiting
    let allow: NSComboButton
    let deny: NSButton
    let note: NSTextField
    weak var owner: Controller?

    init(_ w: Waiting, owner: Controller) {
        item = w
        self.owner = owner
        let width: CGFloat = 318
        let always = NSMenu()
        let a = NSMenuItem(title: "Always allow", action: #selector(AskView.pressAlways), keyEquivalent: "")
        a.image = symbol("checkmark.circle")
        if let rule = w.always {
            if #available(macOS 14.4, *) { a.subtitle = rule } else { a.title = "Always allow: \(rule)" }
        } else {
            a.isEnabled = false
            if #available(macOS 14.4, *) { a.subtitle = "Not offered for this request" }
        }
        always.autoenablesItems = false
        always.addItem(a)
        allow = NSComboButton(title: "Allow once", menu: always, target: nil, action: nil)
        allow.style = .split
        allow.controlSize = .small
        deny = NSButton(title: "Deny", target: nil, action: nil)
        deny.bezelStyle = .push
        deny.controlSize = .small
        note = label("", size: 11, secondary: true)
        super.init(frame: NSRect(x: 0, y: 0, width: width, height: 88))
        a.target = self
        allow.target = self; allow.action = #selector(pressAllow)
        deny.target = self; deny.action = #selector(pressDeny)
        let icon = NSImageView(image: warnImage() ?? NSImage())
        let title = label(w.question, size: 13, weight: .semibold)
        let ago = label(w.ago, size: 11, secondary: true)
        ago.alignment = .right
        let where_ = label("\(w.repo) · \(w.title)", size: 11, secondary: true)
        let account = label(w.account, size: 11, secondary: true)
        // Laid out as a menu row: the state column, the 16 pt image, then the text.
        let x: CGFloat = 45
        icon.frame = NSRect(x: 22, y: 88 - 23, width: 16, height: 16)
        title.frame = NSRect(x: x, y: 88 - 24, width: width - x - 50, height: 17)
        ago.frame = NSRect(x: width - 50, y: 88 - 23, width: 38, height: 14)
        where_.frame = NSRect(x: x, y: 88 - 39, width: width - x - 12, height: 14)
        account.frame = NSRect(x: x, y: 88 - 53, width: width - x - 12, height: 14)
        allow.sizeToFit(); deny.sizeToFit()
        allow.frame = NSRect(x: x - 2, y: 8, width: max(allow.frame.width, 112), height: 24)
        deny.frame = NSRect(x: allow.frame.maxX + 6, y: 8, width: max(deny.frame.width, 64), height: 24)
        note.frame = NSRect(x: deny.frame.maxX + 8, y: 12, width: width - deny.frame.maxX - 16, height: 14)
        for v in [icon, title, ago, where_, account, allow, deny, note] as [NSView] { addSubview(v) }
        if !w.answerable || w.requestId == nil { allow.isEnabled = false; deny.isEnabled = false; note.stringValue = "Answer in VS Code" }
        setAccessibilityLabel("\(w.question) \(w.repo), \(w.title)")
    }
    required init?(coder: NSCoder) { fatalError() }

    @objc func pressAllow() { answer(allow: true, always: false) }
    @objc func pressAlways() { answer(allow: true, always: true) }
    @objc func pressDeny() { answer(allow: false, always: false) }

    func answer(allow yes: Bool, always: Bool) {
        allow.isEnabled = false; deny.isEnabled = false
        note.stringValue = always ? "Always allowing…" : yes ? "Allowing…" : "Denying…"
        owner?.answer(item, allow: yes, always: always) { [weak self] error in
            guard let self else { return }
            if let error { self.note.stringValue = error; self.allow.isEnabled = true; self.deny.isEnabled = true }
            else { self.note.stringValue = always ? "Always allowed" : yes ? "Allowed" : "Denied" }
        }
    }
}

// MARK: - The item

final class Controller: NSObject, NSMenuDelegate {
    let config = Config.load()
    let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    let menu = NSMenu()
    let dot = DotView(frame: NSRect(x: 0, y: 0, width: 7, height: 7))
    var link: Link?
    var nextId = 1
    var pending: [Int: ([String: Any]) -> Void] = [:]
    var snapshot: Snapshot?
    var problem: String?          // why there is no snapshot (the daemon stopped, the wrong daemon)
    var socketPath: String?
    var refreshQueued = false
    var retry: Timer?
    var onSnapshot: [(Snapshot) -> Bool] = []   // --capture/--press wait for a first answer

    override init() {
        super.init()
        menu.delegate = self
        menu.autoenablesItems = false
        item.menu = menu
        item.button?.addSubview(dot)
        dot.isHidden = true
        render()
        connect()
        Timer.scheduledTimer(withTimeInterval: 15, repeats: true) { [weak self] _ in self?.refresh() }
    }

    // The daemon, reached again every 2 s while it is not there.
    func connect() {
        retry?.invalidate(); retry = nil
        if socketPath == nil { socketPath = config.socketPath() }
        guard let path = socketPath, let l = Link(path: path) else {
            snapshot = nil
            problem = "Overseer isn’t running"
            render()
            retry = Timer.scheduledTimer(withTimeInterval: 2, repeats: false) { [weak self] _ in self?.connect() }
            return
        }
        link = l
        l.onMessage = { [weak self] m in self?.received(m) }
        l.onClose = { [weak self] in
            guard let self else { return }
            self.link = nil; self.pending.removeAll()
            self.snapshot = nil; self.problem = "Overseer isn’t running"
            self.render()
            self.retry = Timer.scheduledTimer(withTimeInterval: 2, repeats: false) { [weak self] _ in self?.connect() }
        }
        call("hello", ["client": "menubar"]) { [weak self] r in
            guard let self else { return }
            let instance = (r["result"] as? [String: Any])?["instance"] as? String
            if instance != self.config.instance {
                // The production item never shows a dev daemon, nor a dev item another daemon (AC-212).
                self.problem = "Not \(self.config.instance ?? "the installed Overseer") at \(path)"
                self.render(); self.link?.close(); return
            }
            self.problem = nil
            self.refresh { snap in self.call("events.subscribe", ["after": snap.cursor]) { _ in } }
        }
    }

    func call(_ method: String, _ params: [String: Any] = [:], _ done: @escaping ([String: Any]) -> Void) {
        guard let l = link else { done(["error": ["message": "Overseer isn’t running"]]); return }
        let id = nextId; nextId += 1
        pending[id] = done
        l.send(["id": id, "method": method, "params": params])
    }

    func received(_ m: [String: Any]) {
        if let id = m["id"] as? Int, let done = pending.removeValue(forKey: id) { done(m); return }
        if m["method"] as? String == "event" { queueRefresh() }
    }

    /// Events come in bursts (a turn's output): one refresh per quarter second at most.
    func queueRefresh() {
        if refreshQueued { return }
        refreshQueued = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { [weak self] in self?.refreshQueued = false; self?.refresh() }
    }

    func refresh(_ then: ((Snapshot) -> Void)? = nil) {
        guard link != nil else { return }
        call("menubar.snapshot") { [weak self] r in
            guard let self, let j = r["result"] as? [String: Any] else { return }
            let s = Snapshot(j)
            self.snapshot = s
            self.render()
            then?(s)
            self.onSnapshot = self.onSnapshot.filter { !$0(s) }
        }
    }

    func answer(_ w: Waiting, allow: Bool, always: Bool, _ done: @escaping (String?) -> Void) {
        guard let req = w.requestId else { done("This request is answered in VS Code"); return }
        call("run.permission", ["run_id": w.runId, "request_id": req, "allow": allow, "always": always]) { [weak self] r in
            let error = (r["error"] as? [String: Any])?["message"] as? String
            done(error)
            answered?(r)
            self?.refresh()
        }
    }

    // MARK: Drawing the item and its menu

    func render() {
        guard let button = item.button else { return }
        let waiting = snapshot?.waiting.count ?? 0
        button.image = markImage(dot: waiting > 0, dev: config.dev)
        button.appearsDisabled = snapshot == nil
        let name = config.dev ? "Dev daemon: \(config.instance ?? "")" : "Overseer"
        button.toolTip = snapshot == nil ? "\(name) · not running" : waiting > 0 ? "\(name) · \(waiting) waiting for you" : name
        button.setAccessibilityLabel(button.toolTip)
        dot.isHidden = waiting == 0
        if waiting > 0 {
            // Over the image's cut corner: the image is centred in the button.
            let b = button.bounds
            let ix = (b.width - 18) / 2, iy = (b.height - 18) / 2
            let top = button.isFlipped ? iy - 0.2 : b.height - iy - 7 + 0.2
            dot.frame = NSRect(x: ix + 11.8, y: top, width: 7, height: 7)
        }
        if menuOpen { build() }
    }

    var menuOpen = false
    func menuNeedsUpdate(_ menu: NSMenu) { if menu === self.menu { build() } }
    func menuWillOpen(_ menu: NSMenu) { if menu === self.menu { menuOpen = true; refresh() } }
    func menuDidClose(_ menu: NSMenu) { if menu === self.menu { menuOpen = false } }

    func add(_ menu: NSMenu, _ title: String, subtitle: String? = nil, image: NSImage? = nil, state: Bool = false,
             enabled: Bool = true, action: Selector? = nil, object: Any? = nil) -> NSMenuItem {
        let i = NSMenuItem(title: title, action: action, keyEquivalent: "")
        i.target = self
        i.image = image
        i.state = state ? .on : .off
        i.isEnabled = enabled && action != nil
        i.representedObject = object
        if let subtitle {
            if #available(macOS 14.4, *) { i.subtitle = subtitle }
            else { i.toolTip = subtitle }
        }
        menu.addItem(i)
        return i
    }
    func header(_ menu: NSMenu, _ title: String) {
        if #available(macOS 14, *) { menu.addItem(NSMenuItem.sectionHeader(title: title)) }
        else { let i = NSMenuItem(title: title, action: nil, keyEquivalent: ""); i.isEnabled = false; menu.addItem(i) }
    }
    func grey(_ menu: NSMenu, _ title: String) { _ = add(menu, title, enabled: false) }

    static let shownRequests = 4

    func build() {
        menu.removeAllItems()
        if config.dev { grey(menu, "Dev daemon: \(config.instance ?? "")"); menu.addItem(.separator()) }
        guard let s = snapshot else {
            grey(menu, problem ?? "Overseer isn’t running")
            menu.addItem(.separator())
            _ = add(menu, "Start Overseer", image: symbol("play.fill"), enabled: !config.start.isEmpty, action: #selector(start))
            menu.addItem(.separator())
            _ = add(menu, "Quit", subtitle: "Agents keep running", image: symbol("power"), action: #selector(quit))
            return
        }
        if !s.waiting.isEmpty {
            header(menu, "Needs you")
            for w in s.waiting.prefix(Controller.shownRequests) {
                let i = NSMenuItem()
                i.view = AskView(w, owner: self)
                menu.addItem(i)
            }
            let more = s.waiting.count - Controller.shownRequests
            if more > 0 {
                let dots = NSImage(systemSymbolName: "ellipsis.circle", accessibilityDescription: nil)?
                    .withSymbolConfiguration(NSImage.SymbolConfiguration(paletteColors: [.systemOrange]))
                _ = add(menu, "\(more) more waiting · Show all in Overseer…", image: dots, action: #selector(openURL(_:)),
                        object: "vscode://beelol.overseer/open-center?filter=needs")
            }
            menu.addItem(.separator())
        }
        grey(menu, s.summary)
        menu.addItem(.separator())
        for r in s.repos {
            let i = add(menu, r.name, image: folderImage(waiting: r.waiting), action: #selector(noop))
            if #available(macOS 14, *) { i.badge = NSMenuItemBadge(count: r.count) } else { i.title = "\(r.name)  \(r.count)" }
            let sub = NSMenu(title: r.name)
            sub.autoenablesItems = false
            header(sub, r.count > r.agents.count ? "\(r.agents.count) most recent of \(r.count)" : "Most recent first")
            for a in r.agents {
                _ = add(sub, a.title, subtitle: "\(a.status) · \(a.account)", image: statusImage(a.kind), action: #selector(openAgent(_:)), object: a.runId)
            }
            sub.addItem(.separator())
            var q = URLComponents(string: "vscode://beelol.overseer/open-center")!
            q.queryItems = [URLQueryItem(name: "repo", value: r.name)]
            _ = add(sub, "Show all in Overseer…", image: symbol("list.bullet"), action: #selector(openURL(_:)), object: q.url!.absoluteString)
            i.submenu = sub
        }
        if !s.repos.isEmpty { menu.addItem(.separator()) }
        _ = add(menu, "Voice Mode", image: symbol("waveform"), state: s.voiceEnabled, action: #selector(toggleVoice))
        _ = add(menu, "Mute", image: symbol("mic.slash"), state: s.voiceEnabled && s.voiceMuted, enabled: s.voiceEnabled, action: #selector(toggleMute))
        menu.addItem(.separator())
        _ = add(menu, "Open Overseer", image: symbol("rectangle.split.3x1"), action: #selector(openURL(_:)), object: "vscode://beelol.overseer/open-workspace")
        _ = add(menu, "Talk to Overseer", image: symbol("bubble.left"), action: #selector(openURL(_:)), object: "vscode://beelol.overseer/talk")
        menu.addItem(.separator())
        _ = add(menu, "Quit", subtitle: "Agents keep running", image: symbol("power"), action: #selector(quit))
    }

    // MARK: Actions

    @objc func noop() {}
    @objc func openAgent(_ sender: NSMenuItem) {
        guard let run = sender.representedObject as? String else { return }
        var q = URLComponents(string: "vscode://beelol.overseer/open-agent")!
        q.queryItems = [URLQueryItem(name: "run", value: run)]
        open(q.url!.absoluteString)
    }
    @objc func openURL(_ sender: NSMenuItem) { if let u = sender.representedObject as? String { open(u) } }

    /// VS Code opens the link: the configured one (a dev daemon's own profile), else whichever
    /// VS Code macOS has for vscode:// links.
    func open(_ url: String) {
        opened?(url)
        if let code = config.code {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: code)
            p.arguments = config.codeArgs + ["--open-url", url]
            p.standardOutput = FileHandle.nullDevice; p.standardError = FileHandle.nullDevice
            try? p.run()
        } else if let u = URL(string: url) {
            NSWorkspace.shared.open(u)
        }
    }
    @objc func toggleVoice() { call("voice.set", ["enabled": !(snapshot?.voiceEnabled ?? false)]) { [weak self] _ in self?.refresh() } }
    @objc func toggleMute() { call("voice.set", ["muted": !(snapshot?.voiceMuted ?? false)]) { [weak self] _ in self?.refresh() } }
    @objc func start() {
        guard !config.start.isEmpty else { return }
        // Detached: the daemon must outlive this item (a shell that exits at once leaves it to launchd).
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = ["-c", "\"$@\" </dev/null >/dev/null 2>&1 &", "sh"] + config.start
        var env = ProcessInfo.processInfo.environment
        if !config.dev { for k in env.keys where k.hasPrefix("OVERSEER_") { env.removeValue(forKey: k) } }
        p.environment = env
        try? p.run()
        p.waitUntilExit()
        problem = "Starting Overseer…"
        render()
        retry?.invalidate()
        retry = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: false) { [weak self] _ in self?.connect() }
    }
    @objc func quit() { NSApp.terminate(nil) }
}

// Hooks for the evidence modes.
var opened: ((String) -> Void)?
var answered: (([String: Any]) -> Void)?

// MARK: - Evidence (--capture, --press, --choose): the real menu, opened and used in-process

typealias CreateImage = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?
/// This app's own windows only (the menu and the item), so no screen-recording permission is involved.
func windowImage(_ number: Int) -> CGImage? {
    guard let sym = dlsym(dlopen(nil, RTLD_NOW), "CGWindowListCreateImage") else { return nil }
    let f = unsafeBitCast(sym, to: CreateImage.self)
    return f(.null, 1 << 3, UInt32(number), 1 | 8)?.takeRetainedValue()
}
/// This app's on-screen windows as CoreGraphics sees them (top-left origin): the open menus
/// (layer above the menu bar's) or one window by number.
func ownWindows(_ pick: ([String: Any]) -> Bool) -> [(Int, CGRect)] {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    return list.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == getpid() && pick($0) }
        .compactMap { w in
            guard let n = w[kCGWindowNumber as String] as? Int, let b = w[kCGWindowBounds as String] as? [String: CGFloat] else { return nil }
            return (n, CGRect(x: b["X"] ?? 0, y: b["Y"] ?? 0, width: b["Width"] ?? 0, height: b["Height"] ?? 0))
        }
}
func ownMenuWindows() -> [(Int, CGRect)] { ownWindows { ($0[kCGWindowLayer as String] as? Int ?? 0) > 25 } }

final class Evidence {
    let c: Controller
    let prefix: String?
    let dark: Bool
    init(_ c: Controller) {
        self.c = c
        prefix = arg("--capture")
        dark = (arg("--appearance") ?? "dark") == "dark"
    }

    func run() {
        let when = arg("--when")
        let deadline = Date().addingTimeInterval(20)
        c.onSnapshot.append { [weak self] s in
            guard let self else { return true }
            if let w = when, !s.summary.contains(w) && !(s.waiting.first?.question.contains(w) ?? false) && Date() < deadline { return false }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { self.go() }
            return true
        }
        // A stopped daemon: act on the "not running" menu after the first attempt.
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self] in if let self, self.c.snapshot == nil, !self.started { self.go() } }
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) { fail("no answer from the daemon in 30 s", 3) }
    }

    var started = false
    func go() {
        if started { return }
        started = true
        trace("go: snapshot \(c.snapshot != nil)")
        NSApp.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        c.item.button?.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        c.render()
        c.menuOpen = true
        c.build()
        if let title = arg("--choose-item") { chooseItem(title); return }
        if let repo = arg("--choose") { choose(repo, Int(arg("--index") ?? "0") ?? 0); return }
        // Open the menu under the item, as a click does, and work inside its tracking loop.
        let timer = Timer(timeInterval: 0.45, repeats: false) { [weak self] _ in self?.inside() }
        RunLoop.main.add(timer, forMode: .common)
        guard let button = c.item.button else { fail("no status item") }
        c.menu.popUp(positioning: nil, at: NSPoint(x: -1, y: button.bounds.height + 5), in: button)
    }

    var submenuWanted: String? { arg("--submenu") }

    var mainWindow: Int?
    func inside() {
        mainWindow = ownMenuWindows().first?.0
        trace("inside the menu: \(c.menu.items.count) items, windows \(ownMenuWindows())")
        if let repo = submenuWanted, let idx = c.menu.items.firstIndex(where: { $0.title == repo || $0.title.hasPrefix(repo + "  ") }) {
            // Walk the keyboard to the repository and open its submenu, as the owner would.
            let enabled = c.menu.items.enumerated().filter { $0.element.isEnabled && !$0.element.isSeparatorItem && $0.element.view == nil }.map { $0.offset }
            let steps = (enabled.firstIndex(of: idx) ?? 0) + 1
            for k in 0..<steps { post(125, delay: Double(k) * 0.05) }   // down arrow
            post(124, delay: Double(steps) * 0.05 + 0.05)                // right arrow
            let t = Timer(timeInterval: Double(steps) * 0.05 + 0.6, repeats: false) { [weak self] _ in self?.captureAndAct() }
            RunLoop.main.add(t, forMode: .common)
            return
        }
        captureAndAct()
    }

    func post(_ key: UInt16, delay: Double) {
        let t = Timer(timeInterval: delay, repeats: false) { _ in
            let arrow: String = key == 125 ? "\u{F701}" : "\u{F703}"
            if let e = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.numericPad, .function], timestamp: ProcessInfo.processInfo.systemUptime,
                                        windowNumber: 0, context: nil, characters: arrow, charactersIgnoringModifiers: arrow, isARepeat: false, keyCode: key) {
                NSApp.postEvent(e, atStart: false)
            }
            if let up = NSEvent.keyEvent(with: .keyUp, location: .zero, modifierFlags: [.numericPad, .function], timestamp: ProcessInfo.processInfo.systemUptime,
                                         windowNumber: 0, context: nil, characters: arrow, charactersIgnoringModifiers: arrow, isARepeat: false, keyCode: key) {
                NSApp.postEvent(up, atStart: false)
            }
        }
        RunLoop.main.add(t, forMode: .common)
    }

    func captureAndAct() {
        trace("capture: windows \(ownMenuWindows())")
        if let prefix { write(prefix) }
        if let what = arg("--press") {
            let index = Int(arg("--index") ?? "0") ?? 0
            let asks = c.menu.items.compactMap { $0.view as? AskView }
            guard index < asks.count else { fail("no request \(index) in the menu (\(asks.count) shown)") }
            let v = asks[index]
            answered = { r in
                say(String(data: (try? JSONSerialization.data(withJSONObject: r, options: [.sortedKeys])) ?? Data(), encoding: .utf8) ?? "")
                let t = Timer(timeInterval: 0.6, repeats: false) { _ in
                    if let prefix = self.prefix { self.write(prefix + "-after") }
                    self.c.menu.cancelTracking()
                    DispatchQueue.main.async { exit((r["error"] == nil) ? 0 : 4) }
                }
                RunLoop.main.add(t, forMode: .common)
            }
            switch what {
            case "allow": v.allow.performClick(nil)
            case "deny": v.deny.performClick(nil)
            case "always":
                // The Always allow item under the split button's arrow.
                guard let item = v.allow.menu.items.first, item.isEnabled, let action = item.action else { fail("this request offers no Always allow") }
                NSApp.sendAction(action, to: item.target, from: item)
            default: fail("--press allow|always|deny")
            }
            return
        }
        c.menu.cancelTracking()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { exit(0) }
    }

    func chooseItem(_ title: String) {
        guard let idx = c.menu.items.firstIndex(where: { $0.title == title }) else { fail("no item \(title) in the menu") }
        opened = { url in say(url) }
        c.menu.performActionForItem(at: idx)
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { exit(0) }
    }

    func choose(_ repo: String, _ index: Int) {
        guard let r = c.menu.items.first(where: { $0.submenu?.title == repo }), let sub = r.submenu else { fail("no repository \(repo) in the menu") }
        let agents = sub.items.enumerated().filter { $0.element.action == #selector(Controller.openAgent(_:)) }
        guard index < agents.count else { fail("\(repo) lists \(agents.count) agents") }
        opened = { url in say(url) }
        sub.performActionForItem(at: agents[index].offset)
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { exit(0) }
    }

    /// The item on a menu bar strip, with the open menu (and submenu) below it, as on screen.
    func write(_ prefix: String) {
        let menus = ownMenuWindows().sorted { a, _ in a.0 == mainWindow }   // the main menu first
        guard let button = c.item.button, let main = menus.first else { fail("the menu is not open") }
        let scale: CGFloat = 2
        let images = menus.compactMap { m -> (CGImage, CGRect)? in windowImage(m.0).map { ($0, m.1) } }
        // The menu bar itself is drawn by the system in macOS 26, so the item is drawn from its own
        // button (the same template image and dot AppKit shows), where the menu was opened from it.
        let bsize = button.bounds.size
        let itemRect = CGRect(x: main.1.minX + 1, y: main.1.minY - 5 - bsize.height, width: bsize.width, height: bsize.height)
        var itemImage: CGImage?
        if let rep = button.bitmapImageRepForCachingDisplay(in: button.bounds) {
            button.cacheDisplay(in: button.bounds, to: rep)
            itemImage = rep.cgImage
        }
        trace("item \(itemRect), menus \(menus)")
        let all = images.map { $0.1 } + [itemRect]
        let minX = all.map { $0.minX }.min()! - 24, maxX = all.map { $0.maxX }.max()! + 24
        let top = itemRect.minY
        let maxY = all.map { $0.maxY }.max()! + 24
        let w = Int(((maxX - minX) * scale).rounded()), h = Int(((maxY - top) * scale).rounded())
        guard w > 0, h > 0, w < 20000, h < 20000 else { fail("odd capture size \(w)×\(h)") }
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { fail("no canvas") }
        // A desktop and its menu bar, in screen points with the origin at the top left.
        ctx.translateBy(x: 0, y: CGFloat(h)); ctx.scaleBy(x: scale, y: -scale)
        let W = CGFloat(w) / scale, H = CGFloat(h) / scale
        let wall = dark ? [CGColor(srgbRed: 0.09, green: 0.10, blue: 0.20, alpha: 1), CGColor(srgbRed: 0.16, green: 0.10, blue: 0.27, alpha: 1)]
                        : [CGColor(srgbRed: 0.87, green: 0.91, blue: 0.97, alpha: 1), CGColor(srgbRed: 0.93, green: 0.88, blue: 0.95, alpha: 1)]
        if let grad = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: wall as CFArray, locations: [0, 1]) {
            ctx.drawLinearGradient(grad, start: .zero, end: CGPoint(x: W, y: H), options: [])
        }
        ctx.setFillColor(dark ? CGColor(gray: 0, alpha: 0.18) : CGColor(gray: 1, alpha: 0.30))
        ctx.fill(CGRect(x: 0, y: 0, width: W, height: itemRect.height))
        func draw(_ img: CGImage, _ r: CGRect) {
            ctx.saveGState()
            ctx.translateBy(x: r.minX - minX, y: r.maxY - top)
            ctx.scaleBy(x: 1, y: -1)
            ctx.draw(img, in: CGRect(x: 0, y: 0, width: r.width, height: r.height))
            ctx.restoreGState()
        }
        if let itemImage { draw(itemImage, itemRect) }
        for (img, r) in images.reversed() { draw(img, r) }
        guard let out = ctx.makeImage() else { fail("no image") }
        let rep = NSBitmapImageRep(cgImage: out)
        rep.size = NSSize(width: W, height: H)
        guard let png = rep.representation(using: .png, properties: [:]) else { fail("no PNG") }
        do { try png.write(to: URL(fileURLWithPath: prefix + ".png")) } catch { fail("cannot write \(prefix).png: \(error)") }
        say("wrote \(prefix).png (\(images.count) menu\(images.count == 1 ? "" : "s"))")
    }
}

// MARK: - Login item (scripts/deploy only: tests never register anything)

if let what = arg("--login-item") {
    if #available(macOS 13, *) {
        let service = SMAppService.mainApp
        do {
            switch what {
            case "on": try service.register()
            case "off": try service.unregister()
            case "status": break
            default: fail("--login-item on|off|status")
            }
        } catch { fail("login item \(what): \(error.localizedDescription)", 2) }
        let names: [SMAppService.Status: String] = [.enabled: "enabled", .notRegistered: "not registered", .requiresApproval: "requires approval in System Settings › Login Items", .notFound: "not found"]
        print(names[service.status] ?? "unknown")
        exit(0)
    }
    fail("login items need macOS 13", 2)
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
final class Delegate: NSObject, NSApplicationDelegate {
    var controller: Controller?
    var evidence: Evidence?
    func applicationDidFinishLaunching(_ note: Notification) {
        let c = Controller()
        controller = c
        if arg("--capture") != nil || arg("--press") != nil || arg("--choose") != nil || arg("--choose-item") != nil {
            evidence = Evidence(c)
            evidence?.run()
        }
    }
}
let delegate = Delegate()
app.delegate = delegate
app.run()
