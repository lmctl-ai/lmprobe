package com.example.repo;

import java.util.List;
import java.util.Optional;

public interface UserRepository {
    Optional<User> findById(long id);
    List<User> findAll();
    void save(User user);
}

class JdbcUserRepository implements UserRepository {
    @Override
    public Optional<User> findById(long id) {
        return Optional.empty();
    }

    @Override
    public List<User> findAll() {
        return List.of();
    }

    @Override
    public void save(User user) {
    }
}
