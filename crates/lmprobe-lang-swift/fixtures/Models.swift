import Foundation

protocol Identifiable {
    var id: String { get }
}

struct User: Identifiable {
    let id: String
    let name: String
}

enum Status {
    case active
    case inactive
}

typealias UserId = String

let DEFAULT_TIMEOUT: Int = 30
