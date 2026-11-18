# Required by UniFFI Kotlin + JNA in R8 release builds
-keep class com.sun.jna.** { *; }
-dontwarn java.awt.**
-keep class uniffi.** { *; }
