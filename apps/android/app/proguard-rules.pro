# Required by UniFFI Kotlin + JNA in R8 release builds
-keep class com.sun.jna.** { *; }
-keep interface com.sun.jna.** { *; }
-dontwarn java.awt.**
-keep class uniffi.** { *; }
-keep interface uniffi.** { *; }
-keepattributes *Annotation*,Signature,InnerClasses,EnclosingMethod

# ZXing QR decoder
-dontwarn com.google.zxing.**
