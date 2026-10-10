using System.Collections.Generic;

namespace Lmprobe.Sample;

public interface IRepository
{
    User? ById(long id);
}

public class UserRepository : IRepository
{
    private readonly Dictionary<long, User> _store = new();

    public User? ById(long id) => _store.TryGetValue(id, out var u) ? u : null;

    public override string ToString() => $"UserRepository({_store.Count})";
}
