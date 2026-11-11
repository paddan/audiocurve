import Foundation

public enum Bands {
    public static let count = 12
    public static let centers: [Double] = [125, 250, 500, 750, 1000, 1500, 2000, 3000, 4000, 6000, 8000, 12000]
    /// Geometric means of adjacent centres (11 values).
    public static let crossovers: [Double] = (0..<(count - 1)).map { (centers[$0] * centers[$0 + 1]).squareRoot() }
}
