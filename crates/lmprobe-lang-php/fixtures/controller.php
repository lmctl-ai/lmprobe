<?php

namespace App\Http\Controllers;

use App\Models\User;
use App\Services\AuthService;

#[Route("/users")]
class UserController extends BaseController implements JsonSerializable
{
    public function __construct(
        private AuthService $auth
    ) {}

    #[Get("/{id}")]
    public function show(int $id): User
    {
        return User::find($id);
    }

    public function index(): array
    {
        return User::all();
    }
}
