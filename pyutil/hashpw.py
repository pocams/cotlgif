import sys
from argon2 import PasswordHasher

hasher = PasswordHasher()
password = sys.argv[1]
print(f"Hashing password: {password!r}")
print(hasher.hash(password))
