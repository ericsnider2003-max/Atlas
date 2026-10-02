import CoreLocation
import Foundation
#if canImport(WeatherKit)
import WeatherKit
#endif

/// Apple's weather on this iPhone or iPad, for Atlas's weather answers (Eric,
/// 2 Oct 2026). The core asks with a place's coordinates; this answers in
/// the shape of Apple's REST service, so one reader serves both
/// (`src/applewx.rs`). No key on the phone: the app's WeatherKit capability
/// is the permission. If it fails, the core uses Open-Meteo.
enum AppleWeather {
    static func register() {
        #if canImport(WeatherKit)
        if #available(iOS 16.0, *) {
            atlas_mobile_apple_weather(answer)
            return
        }
        #endif
        atlas_mobile_apple_weather(nil)
    }

    private final class Box: @unchecked Sendable { var json: Data? }

    #if canImport(WeatherKit)
    /// Called from the core's own thread, never the main one.
    private static let answer: atlas_weather_fn = { req, out, len in
        guard let req, let out, len > 1 else { return 4 }
        guard #available(iOS 16.0, *) else { return 3 }
        let data = Data(bytes: req, count: strlen(req))
        guard let body = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let lat = body["lat"] as? Double, let lon = body["lon"] as? Double else { return 4 }
        let box = Box()
        let done = DispatchSemaphore(value: 0)
        Task.detached(priority: .userInitiated) {
            defer { done.signal() }
            guard let both = try? await WeatherService.shared.weather(
                for: CLLocation(latitude: lat, longitude: lon), including: .current, .daily) else { return }
            let (now, daily) = both
            /// "mostlyClear" -> "MostlyClear", as the REST service names them.
            func code(_ c: WeatherCondition) -> String { c.rawValue.prefix(1).uppercased() + c.rawValue.dropFirst() }
            let days: [[String: Any]] = daily.forecast.prefix(2).map { d in
                ["temperatureMax": d.highTemperature.converted(to: .celsius).value,
                 "temperatureMin": d.lowTemperature.converted(to: .celsius).value,
                 "precipitationChance": d.precipitationChance,
                 "conditionCode": code(d.condition)]
            }
            let answer: [String: Any] = [
                "currentWeather": [
                    "temperature": now.temperature.converted(to: .celsius).value,
                    "temperatureApparent": now.apparentTemperature.converted(to: .celsius).value,
                    "windSpeed": now.wind.speed.converted(to: .kilometersPerHour).value,
                    "conditionCode": code(now.condition),
                ],
                "forecastDaily": ["days": days],
            ]
            box.json = try? JSONSerialization.data(withJSONObject: answer)
        }
        done.wait()
        guard let json = box.json else { return 4 }
        let n = min(json.count, len - 1)
        json.withUnsafeBytes { raw in if let b = raw.baseAddress { memcpy(out, b, n) } }
        out[n] = 0
        return 0
    }
    #endif
}
