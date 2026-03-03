const fs = require('fs');

function buildQuery(domain) {
    let header = Buffer.from([0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    let qname = Buffer.alloc(0);
    const parts = domain.split('.');
    for (const part of parts) {
        let len = Buffer.from([part.length]);
        let val = Buffer.from(part);
        qname = Buffer.concat([qname, len, val]);
    }
    qname = Buffer.concat([qname, Buffer.from([0x00])]);
    let tail = Buffer.from([0x00, 0x01, 0x00, 0x01]);
    return Buffer.concat([header, qname, tail]);
}

const lines = fs.readFileSync('benchmark/datasets/top-10k.txt', 'utf8').split('\n');
const out = fs.createWriteStream('benchmark/datasets/top-10k-doh.txt');

for (let i = 0; i < Math.min(lines.length, 1000); i++) {
    const domain = lines[i].trim();
    if (domain && !domain.includes(' ')) {
        const wire = buildQuery(domain);
        const b64 = wire.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, '');
        out.write(b64 + '\n');
    }
}
out.end();
console.log('DoH payloads generated via Node.js');
