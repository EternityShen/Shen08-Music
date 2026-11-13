export ANDROID_NDK_HOME=/home/shen/Work/NDK/android-ndk-r30/

echo "清理环境"

rm -rf ./jniLibs/*
rm -rf ../app/app/src/main/jniLibs/*

cargo ndk --platform 26 -t arm64-v8a -t x86_64 -o jniLibs build --release

echo "Lib编译完成"

mkdir -p ../app/app/src/main/jniLibs

cp -r ./jniLibs/* ../app/app/src/main/jniLibs

echo "Copy JinLibs 完成"