import Testing
@testable import AudioCurveCore

@Test func capsPerFrequency() {
    for f in [125.0, 250, 500] { #expect(TestCalibration.capDBFS(f) == -1) }
    for f in [750.0, 1000, 1500] { #expect(TestCalibration.capDBFS(f) == -3) }
    for f in [2000.0, 3000, 4000, 6000, 8000] { #expect(TestCalibration.capDBFS(f) == -5) }
}

@Test func conversionRoundTrips() {
    let c = TestCalibration.nominal(.inEar)
    for f in Audiogram.standardFrequencies {
        #expect(abs(c.dBFS(fromHL: c.hl(fromDBFS: -50, at: f), at: f) + 50) < 1e-9)
        #expect(c.hl(fromDBFS: -60, at: f) == -60 + 100 - TestCalibration.retspl(.inEar, f))
    }
}

@Test func maxHLIsMultipleOf5AndUnderCap() {
    for t in [Transducer.inEar, .overEar] {
        let c = TestCalibration.nominal(t)
        for f in Audiogram.standardFrequencies {
            #expect(c.maxHL(at: f).truncatingRemainder(dividingBy: 5) == 0)
            #expect(c.dBFS(fromHL: c.maxHL(at: f), at: f) <= TestCalibration.capDBFS(f) + 1e-9)
        }
    }
}

@Test func retsplShape() {   // the tables differ, and both have their minimum in the mid range
    for t in [Transducer.inEar, .overEar] {
        #expect(TestCalibration.retspl(t, 125) > TestCalibration.retspl(t, 1000) + 10)
    }
    #expect(Audiogram.standardFrequencies.contains { TestCalibration.retspl(.inEar, $0) != TestCalibration.retspl(.overEar, $0) })
}

@Test func roundingAndClamp() {
    #expect(TestCalibration.roundedHL(42.4) == 40); #expect(TestCalibration.roundedHL(42.6) == 45)
    #expect(TestCalibration.roundedHL(-30) == -10); #expect(TestCalibration.roundedHL(140) == 120)
}

@Test func defaultTransducerFromName() {
    #expect(TestCalibration.defaultTransducer(deviceName: "My AirPods Pro") == .inEar)
    #expect(TestCalibration.defaultTransducer(deviceName: "WH-1000XM6") == .overEar)
}
