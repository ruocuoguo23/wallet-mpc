#!/bin/bash

set -e # Helps to give error info

# Define color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
CYAN='\033[0;36m'
WHITE='\033[0;37m'
NC='\033[0m' # No Color (reset)

# Function to print colored text
print_colored_text() {
    local color=$1
    local text=$2
    printf "${color}${text}${NC}"
}

ios_targets=(aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios)

option_release=false
option_targets=()
option_all=false
option_archive=false

# Function to print usage
usage() {
    echo "Usage: $0 [--targets aarch64-apple-ios|aarch64-apple-ios-sim...] [--all] [--archive] [--release] [--] [args...]"
    exit 1
}

# Parse options
while [[ $# -gt 0 ]]; do
    case "$1" in
    --release)
        option_release=true
        shift
        ;;
    -t | --targets)
        if [[ -n "$2" && "$2" != -* ]]; then
            option_targets+=("$2")
            shift 2
        else
            echo "Error: --targets requires an argument."
            usage
        fi
        ;;
    --all)
        option_all=true
        shift
        ;;
    --archive)
        option_archive=true
        shift
        ;;
    --)
        shift
        break
        ;;
    -*)
        echo "Unknown option: $1"
        echo
        usage
        ;;
    *)
        break
        ;;
    esac
done

find_target() {
    if [ "$option_all" = true ]; then
        return 0
    fi
    local target
    # Loop through the array
    for target in "${option_targets[@]}"; do
        if [[ "$target" == "$1" ]]; then
            return 0
        fi
    done
    return 1
}

PUBLISH_DIR=publish_libs
BUILD_RESULTS=()

BUILD_PROFILE=debug
BUILD_OPTIONS=

if [ "$option_release" = true ]; then
    BUILD_PROFILE=release
    BUILD_OPTIONS=--release
fi

build_ios() {
    local target=$1
    echo "Building ${target}..."
    cargo build ${BUILD_OPTIONS} -p client --target $target
    mkdir -p ${PUBLISH_DIR}/ios/$target
    cp target/$target/${BUILD_PROFILE}/libclient.a ${PUBLISH_DIR}/ios/$target/
    BUILD_RESULTS+=("${PUBLISH_DIR}/ios/${target}")
    print_colored_text $GREEN "Building ${target} done"
}

# Compile lib
# desktop target
cargo build -p client ${BUILD_OPTIONS}

has_ios_target=false
# iOS
for ios_target in "${ios_targets[@]}"; do
    if find_target $ios_target; then
        build_ios $ios_target
        has_ios_target=true
    fi
done

# UniFfi iOS bindgen
if [ "$has_ios_target" = true ]; then
    cargo run --features=uniffi/cli --bin uniffi-bindgen \
        generate \
        --library target/${BUILD_PROFILE}/libclient.dylib \
        --language swift \
        --out-dir ${PUBLISH_DIR}/ios
fi

print_colored_text $GREEN "\nBuild done.\n\n"

printf "Build results:\n\n"
for result in "${BUILD_RESULTS[@]}"; do
    tree --noreport $result
done

# archive publish libs
if [ "$option_archive" = true ]; then
    print_colored_text $GREEN "\nArchiving...\n\n"
    VERSION=$(git rev-parse --short HEAD)
    tar -czvf ${PUBLISH_DIR}/wallet_mpc_ios-${VERSION}.tar.gz publish_libs/ios
    print_colored_text $GREEN "\nArchive done.\n\n"
fi

