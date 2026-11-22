import SwiftUI
import UIKit

/// Devti's night palette and device "voices" — the same tokens as the desktop Hub and the
/// Android app (docs/DESIGN.md).
enum Theme {
    static let bg = Color(hex: 0x15122A)
    static let bgDeep = Color(hex: 0x0F0C21)
    static let surface = Color(hex: 0x1D1937)
    static let surface2 = Color(hex: 0x262145)
    static let surface3 = Color(hex: 0x312B55)
    static let line = Color(hex: 0xD6D0FF).opacity(0.10)
    static let text = Color(hex: 0xF2F0FB)
    static let textDim = Color(hex: 0xAAA4CA)
    static let textFaint = Color(hex: 0x736D95)
    static let danger = Color(hex: 0xFF6680)
    static let ok = Color(hex: 0x63E4B3)

    static let voiceOrder = ["iris", "aqua", "coral", "mint", "marigold", "rose"]
    static let voices: [String: Color] = [
        "iris": Color(hex: 0xA596FF), "aqua": Color(hex: 0x5AD6F3), "coral": Color(hex: 0xFF8F7E),
        "mint": Color(hex: 0x63E4B3), "marigold": Color(hex: 0xFFCB62), "rose": Color(hex: 0xFF82C4),
    ]

    /// A device's voice, stable everywhere: FNV-1a over its id (same as voiceFor on Android).
    static func voice(_ id: String) -> Color {
        var h: UInt32 = 0x811C_9DC5
        for u in id.utf16 {
            h ^= UInt32(u)
            h = h &* 0x0100_0193
        }
        return voices[voiceOrder[Int(h % UInt32(voiceOrder.count))]] ?? text
    }

    // Bricolage Grotesque for display, Instrument Sans for everything else (both OFL),
    // falling back to the system font if they ever fail to load.
    static func display(_ size: CGFloat, _ weight: Font.Weight = .bold) -> Font {
        custom("Bricolage Grotesque", size, weight, fallbackDesign: .rounded)
    }

    static func ui(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        custom("Instrument Sans", size, weight, fallbackDesign: .default)
    }

    private static func custom(_ family: String, _ size: CGFloat, _ weight: Font.Weight, fallbackDesign: Font.Design) -> Font {
        UIFont.familyNames.contains(family)
            ? Font.custom(family, size: size).weight(weight)
            : Font.system(size: size, weight: weight, design: fallbackDesign)
    }
}

extension Color {
    init(hex: UInt32) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: 1
        )
    }
}

/// A rounded panel on the night background.
struct Panel<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) { content }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(18)
            .background(Theme.surface, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(Theme.line))
    }
}

struct PrimaryButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Theme.ui(17, .semibold))
            .foregroundStyle(Theme.bg)
            .frame(maxWidth: .infinity, minHeight: 54)
            .background(Theme.text, in: Capsule())
            .opacity(configuration.isPressed ? 0.85 : 1)
            .scaleEffect(configuration.isPressed ? 0.98 : 1)
            .animation(.spring(response: 0.25, dampingFraction: 0.7), value: configuration.isPressed)
    }
}

struct QuietButtonStyle: ButtonStyle {
    var active = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Theme.ui(15, .semibold))
            .foregroundStyle(active ? Theme.bg : Theme.text)
            .frame(maxWidth: .infinity, minHeight: 50)
            .padding(.horizontal, 10)
            .background(active ? Theme.text : Theme.surface2, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .opacity(configuration.isPressed ? 0.8 : 1)
            .scaleEffect(configuration.isPressed ? 0.97 : 1)
            .animation(.spring(response: 0.25, dampingFraction: 0.7), value: configuration.isPressed)
    }
}

/// A small dot in a device's voice; hollow when the device is offline.
struct VoiceDot: View {
    var color: Color
    var online: Bool

    var body: some View {
        Circle()
            .fill(online ? color : .clear)
            .overlay(Circle().strokeBorder(online ? color : Theme.textFaint, lineWidth: 2))
            .frame(width: 12, height: 12)
            .shadow(color: online ? color.opacity(0.7) : .clear, radius: 6)
    }
}
