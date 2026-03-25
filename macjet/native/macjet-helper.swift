/**
 * MacJet — Native Swift Helper
 *
 * Queries macOS APIs for window titles, app info, and accessibility data.
 * Much faster than repeated osascript calls — compiled once, reused forever.
 *
 * Usage: macjet-helper [pid1] [pid2] ... or --all or --test
 * Output: JSON array of app context objects
 */

import Foundation
import AppKit

struct AppContext: Codable {
    let pid: Int32
    let name: String
    let bundleId: String?
    let isFrontmost: Bool
    let windows: [WindowInfo]
    let isHidden: Bool
}

struct WindowInfo: Codable {
    let title: String
    let layer: Int
    let isOnScreen: Bool
    let bounds: WindowBounds?
}

struct WindowBounds: Codable {
    let x: Double
    let y: Double
    let width: Double
    let height: Double
}

// MARK: - NSRunningApplication Info

func getRunningApps(pids: [Int32]?) -> [AppContext] {
    let workspace = NSWorkspace.shared
    let runningApps = workspace.runningApplications
    var results: [AppContext] = []

    let targetPIDs: Set<Int32>?
    if let pids = pids {
        targetPIDs = Set(pids)
    } else {
        targetPIDs = nil
    }

    for app in runningApps {
        // Skip if we have a PID filter and this isn't in it
        if let targets = targetPIDs, !targets.contains(app.processIdentifier) {
            continue
        }

        // Only care about regular apps and background apps
        guard app.activationPolicy == .regular || app.activationPolicy == .accessory else {
            continue
        }

        let appName = app.localizedName ?? app.bundleIdentifier ?? "Unknown"
        let bundleId = app.bundleIdentifier
        let pid = app.processIdentifier
        let isFrontmost = app.isActive

        // Get windows for this PID via CGWindowList
        let windows = getWindowsForPID(pid)

        let context = AppContext(
            pid: pid,
            name: appName,
            bundleId: bundleId,
            isFrontmost: isFrontmost,
            windows: windows,
            isHidden: app.isHidden
        )

        results.append(context)
    }

    return results
}

// MARK: - CGWindowList Window Info

func getWindowsForPID(_ pid: Int32) -> [WindowInfo] {
    guard let windowList = CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly, .excludeDesktopElements],
        kCGNullWindowID
    ) as? [[String: Any]] else {
        return []
    }

    var windows: [WindowInfo] = []

    for window in windowList {
        guard let ownerPID = window[kCGWindowOwnerPID as String] as? Int32,
              ownerPID == pid else {
            continue
        }

        let title = window[kCGWindowName as String] as? String ?? ""
        let layer = window[kCGWindowLayer as String] as? Int ?? 0
        let isOnScreen = window[kCGWindowIsOnscreen as String] as? Bool ?? true

        var bounds: WindowBounds? = nil
        if let boundsDict = window[kCGWindowBounds as String] as? [String: Any] {
            bounds = WindowBounds(
                x: boundsDict["X"] as? Double ?? 0,
                y: boundsDict["Y"] as? Double ?? 0,
                width: boundsDict["Width"] as? Double ?? 0,
                height: boundsDict["Height"] as? Double ?? 0
            )
        }

        // Skip empty title windows (menu bar extras, etc.)
        if title.isEmpty && layer != 0 {
            continue
        }

        windows.append(WindowInfo(
            title: title,
            layer: layer,
            isOnScreen: isOnScreen,
            bounds: bounds
        ))
    }

    return windows
}

// MARK: - AX Window Title (Accessibility)

func getAXWindowTitle(pid: Int32) -> String? {
    let app = AXUIElementCreateApplication(pid)

    var value: AnyObject?
    let result = AXUIElementCopyAttributeValue(app, kAXFocusedWindowAttribute as CFString, &value)

    guard result == .success, let window = value else {
        return nil
    }

    var titleValue: AnyObject?
    let titleResult = AXUIElementCopyAttributeValue(window as! AXUIElement, kAXTitleAttribute as CFString, &titleValue)

    guard titleResult == .success, let title = titleValue as? String else {
        return nil
    }

    return title
}

// MARK: - Main

func main() {
    let args = CommandLine.arguments

    if args.contains("--test") {
        // Test mode: verify the helper works
        let apps = getRunningApps(pids: nil)

        let testOutput: [String: Any] = [
            "status": "ok",
            "app_count": apps.count,
            "swift_version": "6.1",
            "accessibility_trusted": AXIsProcessTrusted()
        ]

        if let data = try? JSONSerialization.data(withJSONObject: testOutput, options: .prettyPrinted),
           let string = String(data: data, encoding: .utf8) {
            print(string)
        }
        return
    }

    // Parse PIDs from args, or use --all
    var targetPIDs: [Int32]? = nil
    if !args.contains("--all") && args.count > 1 {
        targetPIDs = args.dropFirst().compactMap { Int32($0) }
        if targetPIDs?.isEmpty == true {
            targetPIDs = nil
        }
    }

    let apps = getRunningApps(pids: targetPIDs)

    // Enrich with AX titles for frontmost / targeted apps
    var enrichedApps: [AppContext] = []
    for var app in apps {
        // Try to get focused window title via AX API
        if let axTitle = getAXWindowTitle(pid: app.pid) {
            // Add the AX title as a window if not already present
            let hasTitle = app.windows.contains { $0.title == axTitle }
            if !hasTitle {
                var windows = app.windows
                windows.insert(WindowInfo(
                    title: axTitle,
                    layer: 0,
                    isOnScreen: true,
                    bounds: nil
                ), at: 0)
                app = AppContext(
                    pid: app.pid,
                    name: app.name,
                    bundleId: app.bundleId,
                    isFrontmost: app.isFrontmost,
                    windows: windows,
                    isHidden: app.isHidden
                )
            }
        }
        enrichedApps.append(app)
    }

    // Output JSON
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]

    if let data = try? encoder.encode(enrichedApps),
       let string = String(data: data, encoding: .utf8) {
        print(string)
    }
}

main()
