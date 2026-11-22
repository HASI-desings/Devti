import Foundation

/// Builds the core's `InputEvent` and `SystemAction` JSON — the same shapes the Android
/// app's `Input.kt` sends (see docs/PROTOCOL.md, Phase 3).
enum Input {
    typealias Event = [String: Any]

    static func move(_ dx: Double, _ dy: Double) -> Event { ["k": "move", "dx": dx, "dy": dy] }
    static func moveTo(_ x: Double, _ y: Double) -> Event { ["k": "move_to", "x": clamp(x), "y": clamp(y)] }
    static func click(_ button: String = "left", count: Int = 1) -> Event { ["k": "click", "button": button, "count": count] }
    static func button(_ button: String, down: Bool) -> Event { ["k": "button", "button": button, "down": down] }
    static func scroll(_ dx: Double, _ dy: Double) -> Event { ["k": "scroll", "dx": dx, "dy": dy] }
    static func text(_ text: String) -> Event { ["k": "text", "text": text] }
    static func key(_ key: String, _ mods: [String] = []) -> Event { ["k": "key", "key": key, "mods": mods] }
    static func media(_ action: String) -> Event { ["k": "media", "action": action] }

    static func encode(_ events: [Event]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: events) else { return "[]" }
        return String(data: data, encoding: .utf8) ?? "[]"
    }

    static func system(_ action: String, percent: Int? = nil) -> String {
        var o: [String: Any] = ["a": action]
        if let p = percent { o["percent"] = max(0, min(100, p)) }
        guard let data = try? JSONSerialization.data(withJSONObject: o) else { return "{}" }
        return String(data: data, encoding: .utf8) ?? "{}"
    }

    /// Touchpad acceleration: slow moves stay precise, fast flicks cross the screen.
    static func accelerate(_ d: Double, speedPointsPerMs: Double) -> Double {
        let gain = speedPointsPerMs < 0.3 ? 1.0 : speedPointsPerMs < 1.2 ? 1.6 : 2.6
        return d * gain
    }

    private static func clamp(_ v: Double) -> Double { max(0, min(1, v)) }
}
