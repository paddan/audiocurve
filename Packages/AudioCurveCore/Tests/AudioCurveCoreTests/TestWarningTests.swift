import Foundation
import Testing
@testable import AudioCurveCore

@Test func noFlagsGiveNoWarnings() {
    #expect(TestWarning.from([]) == [])
}

@Test func perFrequencyFlagsAreGroupedPerEarRightFirstSortedByFrequency() {
    let flags: [TestFlag] = [
        .noResponseAtMax(.right, frequencyHz: 8000),
        .notConverged(.left, frequencyHz: 3000),
        .noResponseAtMax(.left, frequencyHz: 6000),
        .noResponseAtMax(.right, frequencyHz: 125),
        .notConverged(.left, frequencyHz: 500),
    ]
    #expect(TestWarning.from(flags) == [
        .noResponseAtMax(.right, frequenciesHz: [125, 8000]),
        .noResponseAtMax(.left, frequenciesHz: [6000]),
        .notConverged(.left, frequenciesHz: [500, 3000]),
    ])
}

@Test func warningsComeInAFixedOrder() {
    let flags: [TestFlag] = [
        .asymmetry,
        .notConverged(.right, frequencyHz: 1000),
        .noResponseAtMax(.left, frequencyHz: 8000),
        .catchTrialPresses(4),
        .retestMismatch(.left, dB: 15),
        .retestMismatch(.right, dB: 20),
    ]
    #expect(TestWarning.from(flags) == [
        .retestMismatch(.right, dB: 20),
        .retestMismatch(.left, dB: 15),
        .catchTrialPresses,
        .noResponseAtMax(.left, frequenciesHz: [8000]),
        .notConverged(.right, frequenciesHz: [1000]),
        .asymmetry,
    ])
}

@Test func duplicateFrequencyFlagsAreListedOnce() {
    let flags: [TestFlag] = [.notConverged(.right, frequencyHz: 1000), .notConverged(.right, frequencyHz: 1000)]
    #expect(TestWarning.from(flags) == [.notConverged(.right, frequenciesHz: [1000])])
}

@Test func defaultResultNameIsDeviceAndDate() {
    var c = DateComponents()
    (c.year, c.month, c.day, c.hour) = (2026, 11, 16, 12)
    c.timeZone = TimeZone(identifier: "UTC")
    let date = Calendar(identifier: .gregorian).date(from: c)!
    let name = HearingTestRecord.defaultName(deviceName: "AirPods Pro", date: date,
                                             locale: Locale(identifier: "en_GB"),
                                             timeZone: TimeZone(identifier: "UTC")!)
    #expect(name == "AirPods Pro – 16 Nov 2026")
}
