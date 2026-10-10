import Foundation

@available(iOS 13.0, *)
@MainActor
public class Service {
    @objc public func ping() {}

    @available(iOS 14.0, *)
    public func fetch() async {}
}

@objc(LegacyService)
class LegacyService {}
