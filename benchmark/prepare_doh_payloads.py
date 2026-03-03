import base64
import sys
try:
    import dns.message
    import dns.name
except ImportError:
    import subprocess
    subprocess.check_call([sys.executable, "-m", "pip", "install", "dnspython"])
    import dns.message
    import dns.name

def generate_doh_payloads(input_file, output_file, max_lines=10000):
    with open(input_file, 'r') as f_in, open(output_file, 'w') as f_out:
        count = 0
        for line in f_in:
            domain = line.strip()
            if not domain: continue
            
            try:
                # Create a standard A record query
                q = dns.message.make_query(domain, dns.rdatatype.A)
                wire = q.to_wire()
                
                # Base64Url encode it without padding as per RFC 8484
                b64 = base64.urlsafe_b64encode(wire).decode('ascii').rstrip('=')
                f_out.write(b64 + '\n')
            except Exception as e:
                pass
                
            count += 1
            if count >= max_lines: break

if __name__ == '__main__':
    generate_doh_payloads('benchmark/datasets/top-10k.txt', 'benchmark/datasets/top-10k-doh.txt')
    print("DoH payloads generated.")
