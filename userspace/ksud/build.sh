
# 添加 Android 目标架构
#rustup target add  aarch64-linux-android


# 2. 安装 Android NDK
# 下载 NDK (可以从 Android Studio 或直接下载)
# 设置 NDK 环境变量
export ANDROID_NDK_HOME="/Users/ttttt/Library/Android/sdk/ndk/23.1.7779620"  # 替换为你的 NDK 路径
# 4. 如果需要调试信息，可以设置环境变量
export RUST_BACKTRACE=1
export RUST_LOG=debug

# 5. 交叉编译时可能需要的一些环境变量
export TARGET_CC="${ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android31-clang"
export TARGET_CXX="${ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android31-clang++"
export TARGET_AR="${ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-ar"


# 2. 创建 .cargo/config.toml 配置交叉编译
mkdir -p .cargo
cat > .cargo/config.toml << EOF
[target.aarch64-linux-android]
ar = "${ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-ar"
linker = "${ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android31-clang"

[build]
target = "aarch64-linux-android"

# 如果需要静态链接 C++ 标准库
[target.aarch64-linux-android.env]
CXXFLAGS = "-static-libstdc++"
RUSTFLAGS = "-C target-feature=+crt-static"
EOF

# 3. 编译命令
cargo build --release --target aarch64-linux-android