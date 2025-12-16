#!/usr/bin/env python3
"""
Generate Rust constant arrays containing the first N prime numbers.

This script uses the Sieve of Eratosthenes algorithm to generate primes
and outputs them in Rust array format for use in cryptographic operations.
"""

def sieve_of_eratosthenes(limit):
    """
    Generate all prime numbers up to the given limit using the Sieve of Eratosthenes.

    Args:
        limit: Upper bound (inclusive) for prime generation

    Returns:
        List of prime numbers up to limit
    """
    sieve = [True] * (limit + 1)
    sieve[0] = sieve[1] = False

    for i in range(2, int(limit**0.5) + 1):
        if sieve[i]:
            for j in range(i*i, limit + 1, i):
                sieve[j] = False

    return [i for i in range(2, limit + 1) if sieve[i]]

def generate_rust_primes_file(count, output_file):
    """
    Generate a Rust source file containing the first N primes as a constant array.

    Args:
        count: Number of primes to generate
        output_file: Path to output Rust file
    """
    import math

    # Estimate upper limit needed using the prime number theorem
    # The nth prime is approximately n * (ln(n) + ln(ln(n)))
    if count < 6:
        limit = 15
    else:
        limit = int(count * (math.log(count) + math.log(math.log(count)) + 2))

    # Generate primes and take only the first 'count' primes
    primes = sieve_of_eratosthenes(limit)[:count]

    # Ensure we got enough primes
    while len(primes) < count:
        limit = int(limit * 1.5)
        primes = sieve_of_eratosthenes(limit)[:count]

    # Write Rust source file
    with open(output_file, 'w') as f:
        f.write(f"// Auto-generated file containing the first {count} prime numbers\n")
        f.write(f"// Generated using the Sieve of Eratosthenes algorithm\n")
        f.write(f"// Largest prime: {primes[-1]}\n")
        f.write(f"//\n")
        f.write(f"// This table is used for trial division in safe prime generation.\n")
        f.write(f"// More primes = fewer expensive primality tests = faster key generation.\n\n")

        f.write(f"pub const SMALL_PRIMES: [u32; {count}] = [\n")

        # Format with 25 numbers per line for readability
        for i in range(0, len(primes), 25):
            line_primes = primes[i:i+25]
            f.write("    " + ", ".join(map(str, line_primes)) + ",\n")

        f.write("];\n")

    print(f"✅ Generated {output_file}")
    print(f"   - Prime count: {len(primes)}")
    print(f"   - Largest prime: {primes[-1]}")
    print(f"   - File size: ~{len(str(primes))} bytes")

def main():
    """Generate prime number tables of different sizes."""
    import os

    # Get the directory where this script is located
    script_dir = os.path.dirname(os.path.abspath(__file__))
    output_dir = os.path.join(script_dir, "..", "src", "utils")

    print("🔢 Generating prime number tables...\n")

    # Generate 2000 primes (recommended for balanced performance)
    output_2k = os.path.join(output_dir, "small_primes_500k.rs")
    generate_rust_primes_file(500000, output_2k)
    print()

    # Generate 20000 primes (for extreme optimization scenarios)
    output_20k = os.path.join(output_dir, "small_primes_300k.rs")
    generate_rust_primes_file(300000, output_20k)
    print()

    print("✅ All prime tables generated successfully!")
    print("\n📝 Next steps:")
    print("1. Review the generated files in src/utils/")
    print("2. Update src/utils.rs to use the new prime table:")
    print("   - Change: mod small_primes;")
    print("   - To:     mod small_primes_2k as small_primes;")
    print("3. Update generate_safe_prime() to use more primes:")
    print("   - Change: sieve_generate_safe_primes(rng, bits, 135)")
    print("   - To:     sieve_generate_safe_primes(rng, bits, 1000)  // or higher")
    print("4. Rebuild and benchmark to measure performance improvement")

if __name__ == "__main__":
    main()

