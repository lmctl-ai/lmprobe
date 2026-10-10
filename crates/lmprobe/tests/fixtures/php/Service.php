<?php
namespace App\Service;

use Psr\Log\LoggerInterface;
use App\Repository\UserRepository;

#[Route('/api/users')]
class UserService {
    #[Required]
    public function __construct(private LoggerInterface $log) {}
}
