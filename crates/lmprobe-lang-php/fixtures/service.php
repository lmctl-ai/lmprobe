<?php

namespace App\Services;

use function App\Helpers\config;
use const App\Config\DB_HOST;

interface Cacheable
{
    public function cacheKey(): string;
}

trait HasTimestamps
{
    public function createdAt(): string
    {
        return $this->created_at;
    }
}

enum Status: string
{
    case Active = 'active';
    case Inactive = 'inactive';
}

class AuthService implements Cacheable
{
    use HasTimestamps;

    const TOKEN_TTL = 3600;

    public function authenticate(string $email, string $password): bool
    {
        return true;
    }

    public function cacheKey(): string
    {
        return 'auth:' . $this->id;
    }
}
