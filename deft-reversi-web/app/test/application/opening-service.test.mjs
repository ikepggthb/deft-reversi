import test from 'node:test';
import assert from 'node:assert/strict';
import { OpeningService } from '../../application/opening-service.js';

const sampleOpeningText = `
縦取り = F5D6
　虎系 = F5D6C3
　　虎定石 = F5D6C3D3C4
　　　BergTiger = F5D6C3D3C4B5
　　　イエス流 = F5D6C3D3C4B3
//コメント行
斜め取り = F5F6
`;

test('OpeningService.load: parses opening text', () => {
    const service = new OpeningService();
    assert.equal(service.isLoaded, false);

    service.load(sampleOpeningText);

    assert.equal(service.isLoaded, true);
});

test('OpeningService.match: matches opening at start', () => {
    const service = new OpeningService();
    service.load(sampleOpeningText);

    // 縦取り (index 0) の最初の手 F5
    const match = service.match(0, []);
    assert.notEqual(match, null);
    assert.equal(match.name, '縦取り');
    assert.equal(match.nextPosition, 37); // F5
});

test('OpeningService.match: returns next position in sequence', () => {
    const service = new OpeningService();
    service.load(sampleOpeningText);

    // 縦取りでF5を打った後
    const match = service.match(0, ['f5']);
    assert.notEqual(match, null);
    assert.equal(match.name, '縦取り');
    assert.equal(match.nextPosition, 43); // D6
});

test('OpeningService.match: returns null when completed', () => {
    const service = new OpeningService();
    service.load(sampleOpeningText);

    // 縦取りを完走
    const match = service.match(0, ['f5', 'd6']);
    assert.notEqual(match, null);
    assert.equal(match.name, '縦取り');
    assert.equal(match.nextPosition, null); // 完走
});

test('OpeningService.match: returns null when deviated', () => {
    const service = new OpeningService();
    service.load(sampleOpeningText);

    // 縦取りから外れた（F5の後にC4を打った）
    const match = service.match(0, ['f5', 'c4']);
    assert.equal(match, null);
});

test('OpeningService.match: handles longer opening', () => {
    const service = new OpeningService();
    service.load(sampleOpeningText);

    // 虎定石 (index 2)
    // F5D6C3D3C4
    const match1 = service.match(2, ['f5', 'd6', 'c3']);
    assert.notEqual(match1, null);
    assert.equal(match1.name, '虎定石');
    assert.equal(match1.nextPosition, 19); // D3 = row 2, col 3 = 2*8+3 = 19

    const match2 = service.match(2, ['f5', 'd6', 'c3', 'd3', 'c4']);
    assert.notEqual(match2, null);
    assert.equal(match2.name, '虎定石');
    assert.equal(match2.nextPosition, null); // 完走
});

test('OpeningService.match: ignores pass in record', () => {
    const service = new OpeningService();
    service.load(sampleOpeningText);

    // passを含む棋譜でも正しくマッチ
    const match = service.match(0, ['f5', 'pass', 'd6']);
    assert.notEqual(match, null);
    assert.equal(match.name, '縦取り');
    assert.equal(match.nextPosition, null);
});

test('OpeningService.match: returns null for invalid opening index', () => {
    const service = new OpeningService();
    service.load(sampleOpeningText);

    const match1 = service.match(null, ['f5']);
    assert.equal(match1, null);

    const match2 = service.match(999, ['f5']);
    assert.equal(match2, null);
});
