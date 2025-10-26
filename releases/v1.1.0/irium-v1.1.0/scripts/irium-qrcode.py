#!/usr/bin/env python3
"""Generate QR codes for Irium addresses and payment requests."""

import sys
import os
import qrcode
import io
import base64

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

def generate_qr_code(data, output_file=None):
    """Generate QR code for given data."""
    qr = qrcode.QRCode(
        version=1,
        error_correction=qrcode.constants.ERROR_CORRECT_L,
        box_size=10,
        border=4,
    )
    qr.add_data(data)
    qr.make(fit=True)
    
    img = qr.make_image(fill_color="black", back_color="white")
    
    if output_file:
        img.save(output_file)
        print(f"QR code saved to: {output_file}")
    
    return img

def generate_qr_base64(data):
    """Generate QR code as base64 string."""
    qr = qrcode.QRCode(
        version=1,
        error_correction=qrcode.constants.ERROR_CORRECT_L,
        box_size=10,
        border=4,
    )
    qr.add_data(data)
    qr.make(fit=True)
    
    img = qr.make_image(fill_color="black", back_color="white")
    
    # Convert to base64
    buffered = io.BytesIO()
    img.save(buffered, format="PNG")
    img_str = base64.b64encode(buffered.getvalue()).decode()
    
    return img_str

def create_payment_uri(address, amount=None, label=None):
    """Create Irium payment URI."""
    uri = f"irium:{address}"
    params = []
    
    if amount:
        params.append(f"amount={amount}")
    if label:
        params.append(f"label={label}")
    
    if params:
        uri += "?" + "&".join(params)
    
    return uri

def main():
    if len(sys.argv) < 2:
        print("Irium QR Code Generator")
        print("Usage:")
        print("  python3 irium-qrcode.py address <address>")
        print("  python3 irium-qrcode.py payment <address> <amount>")
        print("  python3 irium-qrcode.py payment <address> <amount> <label>")
        return
    
    command = sys.argv[1]
    
    if command == "address":
        if len(sys.argv) < 3:
            print("Error: Address required")
            return
        
        address = sys.argv[2]
        output_file = f"qr_{address[:10]}.png"
        
        generate_qr_code(address, output_file)
        print(f"Address: {address}")
        print(f"QR code saved to: {output_file}")
    
    elif command == "payment":
        if len(sys.argv) < 4:
            print("Error: Address and amount required")
            return
        
        address = sys.argv[2]
        amount = sys.argv[3]
        label = sys.argv[4] if len(sys.argv) > 4 else None
        
        uri = create_payment_uri(address, amount, label)
        output_file = f"qr_payment_{address[:10]}.png"
        
        generate_qr_code(uri, output_file)
        print(f"Payment URI: {uri}")
        print(f"QR code saved to: {output_file}")
    
    else:
        print(f"Unknown command: {command}")

if __name__ == "__main__":
    main()
