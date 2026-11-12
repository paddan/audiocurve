import Testing; import CAudioCurveDSP; @testable import AudioCurveCore
@Test func paramsLayoutMatchesRust() { #expect(MemoryLayout<ac_params>.size == 640) }
@Test func metersLayout() { #expect(MemoryLayout<ac_meters>.size == 44) }
@Test func bands() { #expect(Bands.centers.count == 12 && Bands.crossovers.count == 11) }
