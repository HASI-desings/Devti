import CoreLocation
import Foundation

/// Find my device on iPhone: one location fix while Devti is open (Apple lets apps
/// locate in the background only with "Always" access and a reason to keep running,
/// which Devti doesn't claim). The core adds the IP addresses and battery.
@MainActor
final class Locator: NSObject, CLLocationManagerDelegate {
    private let manager = CLLocationManager()
    private var waiting: [(String) -> Void] = []
    private var timeout: Timer?

    override init() {
        super.init()
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyNearestTenMeters
    }

    var allowed: Bool {
        let s = manager.authorizationStatus
        return s == .authorizedWhenInUse || s == .authorizedAlways
    }

    var undecided: Bool { manager.authorizationStatus == .notDetermined }

    func requestAccess() { manager.requestWhenInUseAuthorization() }

    /// Hands back a `LocationFix` as JSON within ~12 s, with `error` set when there's no fix.
    func fix(_ done: @escaping (String) -> Void) {
        switch manager.authorizationStatus {
        case .denied, .restricted:
            done(Self.json(nil, error: "Devti isn't allowed to use this iPhone's location"))
            return
        case .notDetermined:
            manager.requestWhenInUseAuthorization()
        default:
            break
        }
        waiting.append(done)
        manager.requestLocation()
        timeout?.invalidate()
        timeout = Timer.scheduledTimer(withTimeInterval: 12, repeats: false) { [weak self] _ in
            Task { @MainActor in self?.finish(self?.manager.location, error: "no fix yet: try again outdoors") }
        }
    }

    private func finish(_ loc: CLLocation?, error: String?) {
        timeout?.invalidate()
        let json = Self.json(loc, error: loc == nil ? error : nil)
        let callbacks = waiting
        waiting.removeAll()
        callbacks.forEach { $0(json) }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        let best = locations.min { $0.horizontalAccuracy < $1.horizontalAccuracy }
        Task { @MainActor in self.finish(best, error: nil) }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        Task { @MainActor in self.finish(manager.location, error: error.localizedDescription) }
    }

    private static func json(_ loc: CLLocation?, error: String?) -> String {
        var o: [String: Any] = [:]
        if let loc {
            o["lat"] = loc.coordinate.latitude
            o["lon"] = loc.coordinate.longitude
            if loc.horizontalAccuracy >= 0 { o["accuracy_m"] = loc.horizontalAccuracy }
            o["ts"] = Int64(loc.timestamp.timeIntervalSince1970 * 1000)
            o["source"] = "gps"
        } else {
            o["source"] = "none"
        }
        if let error { o["error"] = error }
        let data = (try? JSONSerialization.data(withJSONObject: o)) ?? Data("{}".utf8)
        return String(decoding: data, as: UTF8.self)
    }
}
