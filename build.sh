#!/bin/bash

# Set the target directory
TARGET_DIR="bin"

# Create the target directory if it doesn't exist
mkdir -p $TARGET_DIR

# Define projects to build
PROJECTS=("client" "sign-service")

# Execute cargo build --release
echo "Building projects in release mode..."
cargo build --release

# Check if the build was successful
# shellcheck disable=SC2181
if [ $? -eq 0 ]; then
    echo "Build successful."

    # Process each project
    for PROJECT_NAME in "${PROJECTS[@]}"; do
        # Set the path to the release binary
        RELEASE_BINARY="target/release/$PROJECT_NAME"

        # Check if the binary file exists
        if [ -f "$RELEASE_BINARY" ]; then
            echo "Copying $PROJECT_NAME binary to $TARGET_DIR..."
            cp "$RELEASE_BINARY" $TARGET_DIR/
            echo "$PROJECT_NAME binary copied to $TARGET_DIR."
        else
            echo "Error: $PROJECT_NAME binary file not found."
            exit 1
        fi
    done

    echo "All binaries have been built and copied successfully."
else
    echo "Build failed."
    exit 1
fi
