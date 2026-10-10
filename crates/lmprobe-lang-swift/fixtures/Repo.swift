import Foundation

class Repository: BaseRepo {
    let name: String

    init(name: String) {
        self.name = name
    }

    override func load() {
        print("loading \(name)")
    }

    override func save() -> Bool {
        return true
    }
}

class CachedRepository: Repository, Cacheable {
    func cache() {}
}
