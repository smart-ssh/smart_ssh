use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::code::alphabet;
use super::format::parse;
use super::*;

// ------------------------------------------------------------ test helpers

/// Fast stand-in for Argon2 that counts its calls.
#[derive(Default)]
struct FakeKdf(AtomicUsize);

impl FakeKdf {
    fn calls(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

impl KeyDeriver for FakeKdf {
    fn derive(
        &self,
        secret: &[u8],
        salt: &[u8],
        p: &KdfParams,
    ) -> Result<Zeroizing<[u8; KEY_LEN]>, BackupError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let mut h = Sha256::new();
        h.update(secret);
        h.update(salt);
        h.update(p.m_cost_kib.to_le_bytes());
        h.update(p.t_cost.to_le_bytes());
        h.update(p.p_cost.to_le_bytes());
        Ok(Zeroizing::new(h.finalize().into()))
    }
}

const PASSPHRASE: &str = "correct horse battery";

fn code_secret() -> (BackupSecret, BackupSecret) {
    let a = RecoveryCode::generate().unwrap();
    let b = RecoveryCode::parse(&a.display_string()).unwrap();
    (BackupSecret::Code(a), BackupSecret::Code(b))
}

fn pass_secret() -> BackupSecret {
    BackupSecret::Passphrase(SecretString::from(PASSPHRASE.to_string()))
}

fn material(kd: &FakeKdf, s: &BackupSecret) -> SlotMaterial {
    SlotMaterial::derive_with(kd, s, KdfParams::WRITE, None).unwrap()
}

fn sections() -> Vec<Section> {
    vec![
        Section::new(1, 1, true, b"servers".to_vec()),
        Section::new(2, 1, true, b"rules".to_vec()),
        Section::new(3, 2, false, b"hint".to_vec()),
    ]
}

fn input<'a>(secs: &'a [Section], slots: &'a [SlotMaterial]) -> SealInput<'a> {
    SealInput {
        sections: secs,
        slots,
        created_at: 1_700_000_000,
        writer_version: "0.8.0",
        extensions: &[],
        kdf: KdfParams::WRITE,
    }
}

fn sealed(kd: &FakeKdf, secrets: &[&BackupSecret]) -> Vec<u8> {
    let slots: Vec<_> = secrets.iter().map(|s| material(kd, s)).collect();
    seal(&input(&sections(), &slots)).unwrap()
}

fn open_fake(kd: &FakeKdf, bytes: &[u8], s: &BackupSecret) -> Result<OpenedBackup, BackupError> {
    open_with(kd, bytes, s, &[1, 2, 3])
}

// Offsets inside the header core.
const OFF_VERSION: usize = 8;
const OFF_M: usize = 11;
const OFF_T: usize = 15;
const OFF_P: usize = 19;
const OFF_SECTION_COUNT: usize = 23;

fn patch_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn core_len(b: &[u8]) -> usize {
    // magic+ver+kdf(1+12)+count(2)+created(8)+writer_len(1)+writer+ext_count(1) (no extensions)
    OFF_SECTION_COUNT + 2 + 8 + 1 + usize::from(b[OFF_SECTION_COUNT + 10]) + 1
}

// ------------------------------------------------------------ round trips

#[test]
fn round_trip_with_code_passphrase_and_both() {
    let kd = FakeKdf::default();
    let (code, code_again) = code_secret();
    let pass = pass_secret();

    for (secrets, openers) in [
        (vec![&code], vec![&code_again]),
        (vec![&pass], vec![&pass]),
        (vec![&code, &pass], vec![&code_again, &pass]),
    ] {
        let file = sealed(&kd, &secrets);
        for opener in openers {
            let opened = open_fake(&kd, &file, opener).unwrap();
            assert_eq!(opened.sections, sections());
            assert_eq!(opened.created_at, 1_700_000_000);
            assert_eq!(opened.writer_version, "0.8.0");
        }
    }
}

#[test]
fn round_trip_with_real_argon2() {
    let (code, code_again) = code_secret();
    let pass = pass_secret();
    let slots = [
        SlotMaterial::derive(&code).unwrap(),
        SlotMaterial::derive(&pass).unwrap(),
    ];
    let file = seal(&input(&sections(), &slots)).unwrap();
    assert_eq!(
        open(&file, &code_again, &[1, 2, 3]).unwrap().sections,
        sections()
    );
    assert_eq!(open(&file, &pass, &[1, 2, 3]).unwrap().sections, sections());
    let wrong = BackupSecret::Passphrase(SecretString::from("another passphrase".to_string()));
    assert_eq!(
        open(&file, &wrong, &[1, 2, 3]).unwrap_err(),
        BackupError::CannotOpen
    );
}

#[test]
fn higher_but_allowed_kdf_parameters_are_readable() {
    let kd = FakeKdf::default();
    let params = KdfParams {
        m_cost_kib: 128 * 1024,
        t_cost: 4,
        p_cost: 2,
    };
    let (code, code_again) = code_secret();
    let slot = SlotMaterial::derive_with(&kd, &code, params, None).unwrap();
    let secs = sections();
    let mut inp = input(&secs, std::slice::from_ref(&slot));
    inp.kdf = params;
    let file = seal(&inp).unwrap();
    assert_eq!(open_fake(&kd, &file, &code_again).unwrap().sections, secs);
}

#[test]
fn every_seal_uses_fresh_key_and_nonces() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let slots = [material(&kd, &pass)];
    let secs = sections();
    let a = seal(&input(&secs, &slots)).unwrap();
    let b = seal(&input(&secs, &slots)).unwrap();
    assert_ne!(a, b);
}

// ------------------------------------------------------------ tampering

#[test]
fn every_bit_flip_fails_and_returns_nothing() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let file = sealed(&kd, &[&pass]);
    assert!(open_fake(&kd, &file, &pass).is_ok());
    for i in 0..file.len() {
        for bit in 0..8 {
            let mut f = file.clone();
            f[i] ^= 1 << bit;
            assert!(
                open_fake(&kd, &f, &pass).is_err(),
                "flip of byte {i} bit {bit} was accepted"
            );
        }
    }
}

#[test]
fn truncation_extension_swap_and_removal_fail() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let file = sealed(&kd, &[&pass]);
    for cut in 0..file.len() {
        assert!(open_fake(&kd, &file[..cut], &pass).is_err(), "cut {cut}");
    }
    let mut longer = file.clone();
    longer.push(0);
    assert!(open_fake(&kd, &longer, &pass).is_err());

    // Locate the section frames.
    let parsed = parse(&file).unwrap();
    let start = parsed.header_len;
    let frames: Vec<&[u8]> = {
        let mut v = Vec::new();
        let mut pos = start;
        for s in &parsed.sections {
            let end = pos + NONCE_LEN_T + 4 + s.ct.len();
            v.push(&file[pos..end]);
            pos = end;
        }
        v
    };
    let mut swapped = file[..start].to_vec();
    swapped.extend_from_slice(frames[1]);
    swapped.extend_from_slice(frames[0]);
    swapped.extend_from_slice(frames[2]);
    assert!(open_fake(&kd, &swapped, &pass).is_err());

    // Remove a section (and fix the count): the count is authenticated.
    let mut removed = file[..start].to_vec();
    removed[OFF_SECTION_COUNT] = 2;
    removed.extend_from_slice(frames[0]);
    removed.extend_from_slice(frames[1]);
    assert!(open_fake(&kd, &removed, &pass).is_err());
}

const NONCE_LEN_T: usize = 24;

#[test]
fn header_core_and_sections_of_two_files_do_not_mix() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let slot = material(&kd, &pass);
    let secs = sections();
    let a = seal(&input(&secs, std::slice::from_ref(&slot))).unwrap();
    let b = seal(&input(&secs, std::slice::from_ref(&slot))).unwrap();
    let pa = parse(&a).unwrap();
    let mut mixed = a[..pa.header_len].to_vec();
    mixed.extend_from_slice(&b[parse(&b).unwrap().header_len..]);
    assert!(open_fake(&kd, &mixed, &pass).is_err());
}

#[test]
fn unused_slot_changes_do_not_matter_when_still_readable() {
    let kd = FakeKdf::default();
    let (code, code_again) = code_secret();
    let pass = pass_secret();
    let file = sealed(&kd, &[&code, &pass]);
    let parsed = parse(&file).unwrap();
    // Flip a byte in the passphrase slot's wrapped key; open with the code.
    let pass_slot = parsed
        .slots
        .iter()
        .find(|s| s.kind == SlotKind::Passphrase)
        .unwrap();
    let off = pass_slot.raw.as_ptr() as usize - file.as_ptr() as usize + pass_slot.raw.len() - 1;
    let mut f = file.clone();
    f[off] ^= 1;
    assert_eq!(
        open_fake(&kd, &f, &code_again).unwrap().sections,
        sections()
    );
    assert!(open_fake(&kd, &f, &pass).is_err());
}

#[test]
fn wrong_code_and_truncated_file_give_the_same_error() {
    let kd = FakeKdf::default();
    let (code, _) = code_secret();
    let (other, _) = code_secret();
    let file = sealed(&kd, &[&code]);
    let wrong = open_fake(&kd, &file, &other).unwrap_err();
    let truncated = open_fake(&kd, &file[..file.len() - 5], &code).unwrap_err();
    let cut_header = open_fake(&kd, &file[..40], &code).unwrap_err();
    assert_eq!(wrong, BackupError::CannotOpen);
    assert_eq!(wrong, truncated);
    assert_eq!(wrong, cut_header);
    assert_eq!(wrong.to_string(), truncated.to_string());
}

#[test]
fn passphrase_secret_does_not_open_code_only_file_and_vice_versa() {
    let kd = FakeKdf::default();
    let (code, _) = code_secret();
    let pass = pass_secret();
    let file = sealed(&kd, &[&code]);
    assert_eq!(
        open_fake(&kd, &file, &pass).unwrap_err(),
        BackupError::CannotOpen
    );
}

// ------------------------------------------------------------ KDF bounds, versions

#[test]
fn kdf_parameters_outside_bounds_are_rejected_without_running_the_kdf() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let file = sealed(&kd, &[&pass]);
    let before = kd.calls();
    let cases: [(usize, u32); 8] = [
        (OFF_M, WRITE_M_COST_KIB - 1),
        (OFF_M, MAX_M_COST_KIB + 1),
        (OFF_M, u32::MAX),
        (OFF_T, WRITE_T_COST - 1),
        (OFF_T, MAX_T_COST + 1),
        (OFF_P, 0),
        (OFF_P, MAX_P_COST + 1),
        (OFF_T, 0),
    ];
    for (off, v) in cases {
        let mut f = file.clone();
        patch_u32(&mut f, off, v);
        assert_eq!(
            open_fake(&kd, &f, &pass).unwrap_err(),
            BackupError::InvalidKdfParameters,
            "offset {off} value {v}"
        );
    }
    assert_eq!(
        kd.calls(),
        before,
        "the KDF must not run for rejected parameters"
    );
    // Counter-proof: a valid open does run the KDF.
    open_fake(&kd, &file, &pass).unwrap();
    assert!(kd.calls() > before);
}

#[test]
fn argon2_deriver_itself_refuses_out_of_bounds_parameters() {
    let bad = KdfParams {
        m_cost_kib: MAX_M_COST_KIB + 1,
        ..KdfParams::WRITE
    };
    assert_eq!(
        Argon2Deriver.derive(b"x", &[0u8; 16], &bad).unwrap_err(),
        BackupError::InvalidKdfParameters
    );
}

#[test]
fn bounds_themselves_are_accepted() {
    let min = KdfParams::WRITE;
    let max = KdfParams {
        m_cost_kib: MAX_M_COST_KIB,
        t_cost: MAX_T_COST,
        p_cost: MAX_P_COST,
    };
    assert!(min.is_acceptable() && max.is_acceptable());
}

#[test]
fn newer_version_is_rejected_before_key_derivation_with_both_versions() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let mut file = sealed(&kd, &[&pass]);
    let before = kd.calls();
    file[OFF_VERSION..OFF_VERSION + 2].copy_from_slice(&7u16.to_le_bytes());
    let err = open_fake(&kd, &file, &pass).unwrap_err();
    assert_eq!(
        err,
        BackupError::NewerVersion {
            file: 7,
            supported: 1
        }
    );
    let text = err.to_string();
    assert!(text.contains('7') && text.contains('1'));
    assert_eq!(kd.calls(), before);
}

#[test]
fn foreign_data_is_not_a_backup() {
    let kd = FakeKdf::default();
    assert_eq!(
        open_fake(&kd, b"SQLite format 3\0........", &pass_secret()).unwrap_err(),
        BackupError::NotABackup
    );
    assert_eq!(kd.calls(), 0);
}

// ------------------------------------------------------------ unknown sections / extensions

#[test]
fn unknown_required_section_rejects_unknown_optional_is_ignored() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let slots = [material(&kd, &pass)];
    let secs = sections(); // type 3 is optional
    let file = seal(&input(&secs, &slots)).unwrap();

    // Caller does not know optional type 3: ignored, rest returned.
    let opened = open_with(&kd, &file, &pass, &[1, 2]).unwrap();
    assert_eq!(opened.sections.len(), 2);
    assert_eq!(opened.ignored_optional_sections, 1);

    // Caller does not know required type 2: whole file rejected.
    assert_eq!(
        open_with(&kd, &file, &pass, &[1, 3]).unwrap_err(),
        BackupError::UnsupportedRequired
    );
}

#[test]
fn extension_fields_are_authenticated_and_required_ones_reject() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let slots = [material(&kd, &pass)];
    let secs = sections();
    let exts = [
        HeaderExtension {
            id: 1,
            required: false,
            data: vec![1, 2, 3],
        },
        HeaderExtension {
            id: 2,
            required: false,
            data: vec![],
        },
    ];
    let mut inp = input(&secs, &slots);
    inp.extensions = &exts;
    let file = seal(&inp).unwrap();
    assert!(open_fake(&kd, &file, &pass).is_ok());

    let req = [HeaderExtension {
        id: 9,
        required: true,
        data: vec![0],
    }];
    inp.extensions = &req;
    let file = seal(&inp).unwrap();
    assert_eq!(
        open_fake(&kd, &file, &pass).unwrap_err(),
        BackupError::UnsupportedRequired
    );

    // Flipping any byte in the extension area fails.
    let mut inp2 = input(&secs, &slots);
    inp2.extensions = &exts;
    let file = seal(&inp2).unwrap();
    let start = core_len(&file);
    let end = parse(&file).unwrap().core.len();
    for i in start..end {
        let mut f = file.clone();
        f[i] ^= 1;
        assert!(open_fake(&kd, &f, &pass).is_err(), "byte {i}");
    }
}

// ------------------------------------------------------------ rewrap

#[test]
fn rewrap_keeps_core_and_sections_identical() {
    let kd = FakeKdf::default();
    let (code, code_again) = code_secret();
    let pass = pass_secret();
    let file = sealed(&kd, &[&code]);
    let parsed = parse(&file).unwrap();
    let (core, tail) = (parsed.core.to_vec(), file[parsed.header_len..].to_vec());

    // Add a passphrase slot: old code keeps working, new passphrase works.
    let added = rewrap_with(&kd, &file, &code_again, &material(&kd, &pass)).unwrap();
    let p2 = parse(&added).unwrap();
    assert_eq!(p2.core, &core[..]);
    assert_eq!(&added[p2.header_len..], &tail[..]);
    assert!(open_fake(&kd, &added, &code_again).is_ok());
    assert!(open_fake(&kd, &added, &pass).is_ok());

    // Replace the code slot with a new code: the old code no longer opens it.
    let (new_code, new_code_again) = code_secret();
    let replaced = rewrap_with(&kd, &added, &pass, &material(&kd, &new_code)).unwrap();
    let p3 = parse(&replaced).unwrap();
    assert_eq!(p3.core, &core[..]);
    assert_eq!(&replaced[p3.header_len..], &tail[..]);
    assert_eq!(p3.slots.len(), 2);
    assert!(open_fake(&kd, &replaced, &new_code_again).is_ok());
    assert!(open_fake(&kd, &replaced, &pass).is_ok());
    assert_eq!(
        open_fake(&kd, &replaced, &code_again).unwrap_err(),
        BackupError::CannotOpen
    );
}

#[test]
fn rewrap_requires_the_old_secret_and_ignores_slots_of_other_files() {
    let kd = FakeKdf::default();
    let (code, _) = code_secret();
    let (other, _) = code_secret();
    let pass = pass_secret();
    let file = sealed(&kd, &[&code]);
    assert_eq!(
        rewrap_with(&kd, &file, &other, &material(&kd, &pass)).unwrap_err(),
        BackupError::CannotOpen
    );
    // A slot of another file spliced in does not open this file's content.
    let (c2, c2_again) = code_secret();
    let foreign = sealed(&kd, &[&c2]);
    let pf = parse(&foreign).unwrap();
    let pf_slot = &pf.slots[0];
    let pm = parse(&file).unwrap();
    let mut spliced = pm.core.to_vec();
    spliced.push(1);
    spliced.extend_from_slice(pf_slot.raw);
    spliced.extend_from_slice(&file[pm.header_len..]);
    assert!(open_fake(&kd, &spliced, &c2_again).is_err());
}

// ------------------------------------------------------------ recovery code

#[test]
fn generated_codes_parse_back_and_ignore_case_and_separators() {
    for _ in 0..50 {
        let code = RecoveryCode::generate().unwrap();
        let shown = code.display_string();
        assert_eq!(shown.len(), 30 + 5);
        assert!(shown.split('-').all(|g| g.len() == 5));
        let parsed = RecoveryCode::parse(&shown).unwrap();
        assert_eq!(parsed.canonical(), code.canonical());
        let messy = format!(" {} ", shown.to_lowercase().replace('-', " - "));
        assert_eq!(
            RecoveryCode::parse(&messy).unwrap().canonical(),
            code.canonical()
        );
        let compact = shown.replace('-', "");
        assert_eq!(
            RecoveryCode::parse(&compact).unwrap().canonical(),
            code.canonical()
        );
    }
    assert_ne!(
        RecoveryCode::generate().unwrap().canonical(),
        RecoveryCode::generate().unwrap().canonical()
    );
}

#[test]
fn code_has_at_least_128_bits() {
    // 28 data characters of a 32-symbol alphabet.
    let code = RecoveryCode::generate().unwrap();
    assert!((code.canonical().len() - 2) * 5 >= 128);
    assert_eq!(alphabet().len(), 32);
    assert!(!alphabet().iter().any(|c| b"ILOU".contains(c)));
}

#[test]
fn every_single_character_change_and_adjacent_swap_is_a_typo() {
    let code = RecoveryCode::generate().unwrap();
    let canon: Vec<char> = code.canonical().chars().collect();
    for i in 0..canon.len() {
        for &a in alphabet() {
            if a as char == canon[i] {
                continue;
            }
            let mut c = canon.clone();
            c[i] = a as char;
            let s: String = c.into_iter().collect();
            assert_eq!(
                RecoveryCode::parse(&s).unwrap_err(),
                BackupError::CodeTypo,
                "pos {i}"
            );
        }
    }
    for i in 0..canon.len() - 1 {
        if canon[i] == canon[i + 1] {
            continue;
        }
        let mut c = canon.clone();
        c.swap(i, i + 1);
        let s: String = c.into_iter().collect();
        assert_eq!(
            RecoveryCode::parse(&s).unwrap_err(),
            BackupError::CodeTypo,
            "swap {i}"
        );
    }
}

#[test]
fn malformed_codes_are_typos() {
    let code = RecoveryCode::generate().unwrap().display_string();
    assert_eq!(RecoveryCode::parse("").unwrap_err(), BackupError::CodeTypo);
    assert_eq!(
        RecoveryCode::parse(&code[..20]).unwrap_err(),
        BackupError::CodeTypo
    );
    assert_eq!(
        RecoveryCode::parse(&format!("{}A", *code)).unwrap_err(),
        BackupError::CodeTypo
    );
    assert_eq!(
        RecoveryCode::parse(&code.replace('-', "!")).unwrap_err(),
        BackupError::CodeTypo
    );
    assert_eq!(
        RecoveryCode::parse("ÄÖÜ💥").unwrap_err(),
        BackupError::CodeTypo
    );
}

#[test]
fn typo_is_not_reported_as_wrong_code() {
    // A typo never reaches the file: parse fails first with a distinct error.
    let code = RecoveryCode::generate().unwrap().display_string();
    let mut chars: Vec<char> = code.chars().collect();
    chars[0] = if chars[0] == '0' { '1' } else { '0' };
    let typo: String = chars.into_iter().collect();
    let err = RecoveryCode::parse(&typo).unwrap_err();
    assert_ne!(err, BackupError::CannotOpen);
}

#[test]
fn passphrase_rule_is_twelve_characters() {
    assert_eq!(
        validate_passphrase(&SecretString::from("12345678901".to_string())).unwrap_err(),
        BackupError::PassphraseTooShort
    );
    assert!(validate_passphrase(&SecretString::from("123456789012".to_string())).is_ok());
    assert!(validate_passphrase(&SecretString::from("äöüäöüäöüäöü".to_string())).is_ok());
    assert!(SlotMaterial::from_passphrase(SecretString::from("short".to_string())).is_err());
}

// ------------------------------------------------------------ secrets in Debug

#[test]
fn debug_output_never_contains_secrets() {
    let kd = FakeKdf::default();
    let code = RecoveryCode::generate().unwrap();
    let shown = code.display_string().to_string();
    let canon = code.canonical().to_string();
    let secret = BackupSecret::Code(code);
    let pass = pass_secret();
    let slot = material(&kd, &secret);
    let section = Section::new(1, 1, true, b"TOP-SECRET-PAYLOAD".to_vec());
    let rendered = [
        format!("{secret:?}"),
        format!("{:?}", RecoveryCode::parse(&shown).unwrap()),
        format!("{pass:?}"),
        format!("{slot:?}"),
        format!("{section:?}"),
        format!(
            "{:#?}",
            OpenedBackup {
                sections: vec![section],
                created_at: 0,
                writer_version: String::new(),
                ignored_optional_sections: 0,
            }
        ),
        format!("{:?}", BackupError::CannotOpen),
    ]
    .join("\n");
    for needle in [&shown, &canon, PASSPHRASE, "TOP-SECRET-PAYLOAD"] {
        assert!(!rendered.contains(needle), "Debug leaked {needle}");
    }
    let wrapping_hex: String = slot
        .wrapping_key
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert!(!rendered.contains(&wrapping_hex));
}

#[test]
fn plaintext_does_not_appear_in_the_file() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let secs = vec![Section::new(1, 1, true, b"needle-needle-needle".to_vec())];
    let slots = [material(&kd, &pass)];
    let file = seal(&input(&secs, &slots)).unwrap();
    assert!(!file.windows(6).any(|w| w == b"needle"));
}

// ------------------------------------------------------------ F12 limits

#[test]
fn section_count_limit() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let slots = [material(&kd, &pass)];
    let many = |n: usize| -> Vec<Section> {
        (0..n)
            .map(|i| Section::new(1, 1, false, vec![i as u8]))
            .collect()
    };
    let at = many(MAX_SECTIONS);
    let file = seal(&input(&at, &slots)).unwrap();
    assert_eq!(
        open_with(&kd, &file, &pass, &[1]).unwrap().sections.len(),
        MAX_SECTIONS
    );
    let over = many(MAX_SECTIONS + 1);
    assert_eq!(
        seal(&input(&over, &slots)).unwrap_err(),
        BackupError::LimitExceeded(Limit::SectionCount)
    );
    // On read: count field 65 is rejected as a limit, 64 is not.
    let mut f = file.clone();
    f[OFF_SECTION_COUNT..OFF_SECTION_COUNT + 2].copy_from_slice(&65u16.to_le_bytes());
    assert_eq!(
        open_with(&kd, &f, &pass, &[1]).unwrap_err(),
        BackupError::LimitExceeded(Limit::SectionCount)
    );
}

#[test]
fn key_slot_limit() {
    let kd = FakeKdf::default();
    let (code, code_again) = code_secret();
    let pass = pass_secret();
    let file = sealed(&kd, &[&code, &pass]);
    let p = parse(&file).unwrap();
    let build = |n: usize| -> Vec<u8> {
        let mut v = p.core.to_vec();
        v.push(n as u8);
        for i in 0..n {
            // Alternate so that the code slot is present once at least.
            v.extend_from_slice(p.slots[i % 2].raw);
        }
        v.extend_from_slice(&file[p.header_len..]);
        v
    };
    assert!(open_fake(&kd, &build(MAX_KEY_SLOTS), &code_again).is_ok());
    assert_eq!(
        open_fake(&kd, &build(MAX_KEY_SLOTS + 1), &code_again).unwrap_err(),
        BackupError::LimitExceeded(Limit::KeySlotCount)
    );
    assert_eq!(
        open_fake(&kd, &build(0), &code_again).unwrap_err(),
        BackupError::CannotOpen
    );
    // seal: more than 8 slots is refused, as is a duplicate kind.
    let slots: Vec<_> = (0..9).map(|_| material(&kd, &pass)).collect();
    let secs = sections();
    assert_eq!(
        seal(&input(&secs, &slots)).unwrap_err(),
        BackupError::LimitExceeded(Limit::KeySlotCount)
    );
    assert!(matches!(
        seal(&input(&secs, &slots[..2])).unwrap_err(),
        BackupError::InvalidInput(_)
    ));
}

#[test]
fn header_size_limit() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let slots = [material(&kd, &pass)];
    let secs = sections();
    let base = seal(&input(&secs, &slots)).unwrap();
    let base_header = parse(&base).unwrap().header_len;

    // Grow the header with 10 extensions to exactly `target` bytes.
    let build = |target: usize| -> Result<Vec<u8>, BackupError> {
        let extra = target - base_header;
        let n = 10;
        let data_total = extra - n * 5;
        let mut exts = Vec::new();
        for i in 0..n {
            let share = data_total / n + usize::from(i < data_total % n);
            exts.push(HeaderExtension {
                id: i as u16,
                required: false,
                data: vec![0; share],
            });
        }
        let mut inp = input(&secs, &slots);
        inp.extensions = &exts;
        seal(&inp)
    };
    let at = build(MAX_HEADER_BYTES).unwrap();
    assert_eq!(parse(&at).unwrap().header_len, MAX_HEADER_BYTES);
    assert!(open_fake(&kd, &at, &pass).is_ok());
    assert_eq!(
        build(MAX_HEADER_BYTES + 1).unwrap_err(),
        BackupError::LimitExceeded(Limit::HeaderSize)
    );
}

#[test]
fn section_size_limit() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let slots = [material(&kd, &pass)];
    let max_payload = MAX_SECTION_CIPHERTEXT_BYTES - 16 - 5;
    let at = vec![Section::new(1, 1, true, vec![7u8; max_payload])];
    let file = seal(&input(&at, &slots)).unwrap();
    let opened = open_with(&kd, &file, &pass, &[1]).unwrap();
    assert_eq!(opened.sections[0].payload.len(), max_payload);
    drop(opened);
    let over = vec![Section::new(1, 1, true, vec![7u8; max_payload + 1])];
    assert_eq!(
        seal(&input(&over, &slots)).unwrap_err(),
        BackupError::LimitExceeded(Limit::SectionSize)
    );
    // On read: a declared length one above the limit is refused as a limit,
    // exactly at the limit (but without that much data) as a damaged file –
    // and no memory is reserved for either.
    let small = sealed(&kd, &[&pass]);
    let p = parse(&small).unwrap();
    let len_off = p.header_len + 24;
    for (declared, expected) in [
        (
            MAX_SECTION_CIPHERTEXT_BYTES as u32 + 1,
            BackupError::LimitExceeded(Limit::SectionSize),
        ),
        (MAX_SECTION_CIPHERTEXT_BYTES as u32, BackupError::CannotOpen),
        (u32::MAX, BackupError::LimitExceeded(Limit::SectionSize)),
    ] {
        let mut f = small.clone();
        patch_u32(&mut f, len_off, declared);
        assert_eq!(open_fake(&kd, &f, &pass).unwrap_err(), expected);
    }
}

#[test]
fn file_size_limit() {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let at = vec![0u8; MAX_FILE_BYTES];
    assert_eq!(
        open_fake(&kd, &at, &pass).unwrap_err(),
        BackupError::NotABackup
    );
    let over = vec![0u8; MAX_FILE_BYTES + 1];
    assert_eq!(
        open_fake(&kd, &over, &pass).unwrap_err(),
        BackupError::LimitExceeded(Limit::FileSize)
    );
    assert_eq!(kd.calls(), 0);
}

// ------------------------------------------------------------ robustness

fn fuzz_base() -> (FakeKdf, BackupSecret, Vec<u8>) {
    let kd = FakeKdf::default();
    let pass = pass_secret();
    let (code, _) = code_secret();
    let file = sealed(&kd, &[&pass, &code]);
    (kd, pass, file)
}

/// Fixed bound for the fuzz inputs; decryption allocates at most the input
/// size, so memory stays within a small multiple of this.
const FUZZ_MAX_INPUT: usize = 4096;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..FUZZ_MAX_INPUT)) {
        let (kd, pass, _) = fuzz_base();
        let _ = open_fake(&kd, &bytes, &pass);
    }

    #[test]
    fn random_bytes_behind_the_magic_never_panic(
        bytes in proptest::collection::vec(any::<u8>(), 0..FUZZ_MAX_INPUT)
    ) {
        let (kd, pass, _) = fuzz_base();
        let mut input = MAGIC.to_vec();
        input.extend_from_slice(&1u16.to_le_bytes());
        input.push(1);
        input.extend_from_slice(&bytes);
        let _ = open_fake(&kd, &input, &pass);
    }

    #[test]
    fn mutated_valid_files_never_panic_and_never_succeed_with_changed_content(
        edits in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6),
        cut in proptest::option::of(any::<prop::sample::Index>()),
        extra in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        let (kd, pass, file) = fuzz_base();
        let mut f = file.clone();
        for (i, v) in &edits {
            let idx = i.index(f.len());
            f[idx] = *v;
        }
        if let Some(c) = cut {
            f.truncate(c.index(f.len() + 1));
        }
        f.extend_from_slice(&extra);
        prop_assert!(f.len() < FUZZ_MAX_INPUT * 4);
        if let Ok(opened) = open_fake(&kd, &f, &pass) {
            // Only a change confined to the unused slot may succeed, and then
            // the content is exactly the original.
            prop_assert_eq!(opened.sections, sections());
        }
    }
}
