import AudioToolbox
import AudioCurveCore

public enum ParamAddress: AUParameterAddress {
    case strength = 0, mpo = 1, outputGain = 2, mode = 3, compression = 4, limiter = 5, inputGain = 6

    var identifier: String {
        switch self {
        case .strength: "strength"
        case .mpo: "mpo"
        case .outputGain: "outputGain"
        case .mode: "mode"
        case .compression: "compression"
        case .limiter: "limiter"
        case .inputGain: "inputGain"
        }
    }
}

/// Plain values behind the parameter tree. Strength is a percentage (0...100).
struct ParamValues: Sendable {
    var strength: Double = 50
    var mpo: Double = -3
    var outputGain: Double = 0
    var mode: Int = 0
    /// 1 = on, 0 = off (lab switches).
    var compression: Int = 1
    var limiter: Int = 1
    var inputGain: Double = 0

    subscript(a: ParamAddress) -> Double {
        get {
            switch a {
            case .strength: strength
            case .mpo: mpo
            case .outputGain: outputGain
            case .mode: Double(mode)
            case .compression: Double(compression)
            case .limiter: Double(limiter)
            case .inputGain: inputGain
            }
        }
        set {
            switch a {
            case .strength: strength = newValue
            case .mpo: mpo = newValue
            case .outputGain: outputGain = newValue
            case .mode: mode = Int(newValue.rounded())
            case .compression: compression = newValue >= 0.5 ? 1 : 0
            case .limiter: limiter = newValue >= 0.5 ? 1 : 0
            case .inputGain: inputGain = newValue
            }
        }
    }
}

enum Parameters {
    static func makeTree() -> AUParameterTree {
        let d = ParamValues()
        let strength = AUParameterTree.createParameter(
            withIdentifier: ParamAddress.strength.identifier, name: "Styrka",
            address: ParamAddress.strength.rawValue, min: 0, max: 100, unit: .percent,
            unitName: nil, flags: [.flag_IsReadable, .flag_IsWritable], valueStrings: nil,
            dependentParameters: nil)
        strength.value = AUValue(d.strength)
        let mpo = AUParameterTree.createParameter(
            withIdentifier: ParamAddress.mpo.identifier, name: "Maxnivå",
            address: ParamAddress.mpo.rawValue, min: -20, max: -1, unit: .decibels,
            unitName: nil, flags: [.flag_IsReadable, .flag_IsWritable], valueStrings: nil,
            dependentParameters: nil)
        mpo.value = AUValue(d.mpo)
        let input = AUParameterTree.createParameter(
            withIdentifier: ParamAddress.inputGain.identifier, name: "Innivå",
            address: ParamAddress.inputGain.rawValue, min: -30, max: 12, unit: .decibels,
            unitName: nil, flags: [.flag_IsReadable, .flag_IsWritable], valueStrings: nil,
            dependentParameters: nil)
        input.value = AUValue(d.inputGain)
        let gain = AUParameterTree.createParameter(
            withIdentifier: ParamAddress.outputGain.identifier, name: "Utnivå",
            address: ParamAddress.outputGain.rawValue, min: -48, max: 12, unit: .decibels,
            unitName: nil, flags: [.flag_IsReadable, .flag_IsWritable], valueStrings: nil,
            dependentParameters: nil)
        gain.value = AUValue(d.outputGain)
        let mode = AUParameterTree.createParameter(
            withIdentifier: ParamAddress.mode.identifier, name: "Läge",
            address: ParamAddress.mode.rawValue, min: 0, max: 1, unit: .indexed,
            unitName: nil, flags: [.flag_IsReadable, .flag_IsWritable],
            valueStrings: ["Tal", "Musik"], dependentParameters: nil)
        mode.value = AUValue(d.mode)
        let compression = AUParameterTree.createParameter(
            withIdentifier: ParamAddress.compression.identifier, name: "Kompression",
            address: ParamAddress.compression.rawValue, min: 0, max: 1, unit: .boolean,
            unitName: nil, flags: [.flag_IsReadable, .flag_IsWritable], valueStrings: nil,
            dependentParameters: nil)
        compression.value = AUValue(d.compression)
        let limiter = AUParameterTree.createParameter(
            withIdentifier: ParamAddress.limiter.identifier, name: "Limiter",
            address: ParamAddress.limiter.rawValue, min: 0, max: 1, unit: .boolean,
            unitName: nil, flags: [.flag_IsReadable, .flag_IsWritable], valueStrings: nil,
            dependentParameters: nil)
        limiter.value = AUValue(d.limiter)
        return AUParameterTree.createTree(withChildren: [strength, mpo, gain, mode, compression, limiter, input])
    }
}
