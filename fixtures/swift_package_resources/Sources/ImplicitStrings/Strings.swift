import Foundation

public func checkImplicitStrings() {
    precondition(NSLocalizedString("greeting", bundle: .module, comment: "") == "Hello")
    let german = Bundle.module.path(forResource: "Localizable", ofType: "strings", inDirectory: nil, forLocalization: "de")
    precondition(german.map { NSDictionary(contentsOfFile: $0)?["greeting"] as? String } == "Hallo")
}
