import SwiftUI
import UIKit

/// Native `UIImagePickerController` wrapper for capturing camera photos directly from the Composer.
struct CameraPickerView: UIViewControllerRepresentable {
    let onCapture: (UIImage) -> Void
    @Environment(\.dismiss) private var dismiss

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = .camera
        picker.cameraCaptureMode = .photo
        picker.allowsEditing = false
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ uiViewController: UIImagePickerController, context: Context) {}

    final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        private let parent: CameraPickerView

        init(_ parent: CameraPickerView) {
            self.parent = parent
        }

        func imagePickerController(
            _ picker: UIImagePickerController,
            didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]
        ) {
            let captured = info[.originalImage] as? UIImage
            parent.dismiss()
            if let image = captured {
                parent.onCapture(image)
            }
        }

        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
            parent.dismiss()
        }
    }
}

/// Off-main-actor image downscaling, EXIF orientation baking, and byte-size formatting helpers.
enum OrbitAttachmentNormalizer {
    static func normalizeImage(
        _ image: UIImage,
        maxDimension: CGFloat = 2048,
        quality: CGFloat = 0.85
    ) -> (uploadData: Data, previewData: Data)? {
        let originalSize = image.size
        guard originalSize.width > 0, originalSize.height > 0 else { return nil }

        let pixelWidth = originalSize.width * max(1.0, image.scale)
        let pixelHeight = originalSize.height * max(1.0, image.scale)
        let maxSide = max(pixelWidth, pixelHeight)
        let scale = maxSide > maxDimension ? (maxDimension / maxSide) : 1.0
        let targetSize = CGSize(
            width: max(1, (pixelWidth * scale).rounded()),
            height: max(1, (pixelHeight * scale).rounded())
        )

        let hasAlpha: Bool = {
            guard let cgImage = image.cgImage else { return false }
            switch cgImage.alphaInfo {
            case .first, .last, .premultipliedFirst, .premultipliedLast, .alphaOnly:
                return true
            default:
                return false
            }
        }()

        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1.0
        format.opaque = !hasAlpha

        let normalized = UIGraphicsImageRenderer(size: targetSize, format: format).image { _ in
            image.draw(in: CGRect(origin: .zero, size: targetSize))
        }
        let uploadData: Data? = hasAlpha
            ? (normalized.pngData() ?? normalized.jpegData(compressionQuality: quality))
            : normalized.jpegData(compressionQuality: quality)
        guard let uploadData else {
            return nil
        }

        let thumbMax: CGFloat = 160
        let thumbScale = maxSide > thumbMax ? (thumbMax / maxSide) : 1.0
        let thumbSize = CGSize(
            width: max(1, (pixelWidth * thumbScale).rounded()),
            height: max(1, (pixelHeight * thumbScale).rounded())
        )
        let thumbImage = UIGraphicsImageRenderer(size: thumbSize, format: format).image { _ in
            normalized.draw(in: CGRect(origin: .zero, size: thumbSize))
        }
        let previewData = (hasAlpha ? thumbImage.pngData() : thumbImage.jpegData(compressionQuality: 0.72)) ?? uploadData

        return (uploadData: uploadData, previewData: previewData)
    }

    static func normalizeImageData(
        _ rawData: Data,
        maxDimension: CGFloat = 2048,
        quality: CGFloat = 0.85
    ) -> (uploadData: Data, previewData: Data)? {
        guard let image = UIImage(data: rawData) else { return nil }
        return normalizeImage(image, maxDimension: maxDimension, quality: quality)
    }

    static func formatByteCount(_ bytes: Int) -> String {
        if bytes < 1024 {
            return "\(max(bytes, 1)) B"
        }
        if bytes < 1024 * 1024 {
            let kb = max(1, Int((Double(bytes) / 1024.0).rounded()))
            return "\(kb) KB"
        }
        let mb = Double(bytes) / (1024.0 * 1024.0)
        return String(format: "%.1f MB", mb)
    }
}
