import http from 'k6/http';
import { check } from 'k6';
import { SharedArray } from 'k6/data';

const payloads = new SharedArray('payloads', function () {
    return open('./datasets/top-10k-doh.txt').split('\n').filter(Boolean);
});

export const options = {
    scenarios: {
        doh_stress: {
            executor: 'constant-vus',
            vus: 100,
            duration: '30s',
        }
    }
};

export default function () {
    const payload = payloads[Math.floor(Math.random() * payloads.length)];

    // Using GET request with base64url encoded dns parameter
    const res = http.get(`http://127.0.0.1:8080/dns-query?dns=${payload}`, {
        headers: {
            'accept': 'application/dns-message',
        }
    });

    check(res, {
        'status is 200': (r) => r.status === 200,
        'content-type is right': (r) => r.headers['Content-Type'] === 'application/dns-message',
    });
}
