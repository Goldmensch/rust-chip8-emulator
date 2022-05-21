use simple:: {Window, Rect, Key};
use std:: {path::Path, fs::File, io::Read, env, vec::Vec, thread::sleep, time};
use bit_iter::BitIter;
use rand::random;
use rodio::{OutputStream, Sink, source::SineWave};

const DISPLAY_HEIGHT: u16 = 32;
const DISPLAY_WIDTH: u16 = 64;
const DISPLAY_SIZE: usize = (DISPLAY_HEIGHT * DISPLAY_WIDTH) as usize;
const FONTSET_OFFSET: u8 = 0x50;

const FONTSET: [u8; 80] =
    [
  0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
  0x20, 0x60, 0x20, 0x20, 0x70, // 1
  0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
  0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
  0x90, 0x90, 0xF0, 0x10, 0x10, // 4
  0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
  0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
  0xF0, 0x10, 0x20, 0x40, 0x40, // 7
  0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
  0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
  0xF0, 0x90, 0xF0, 0x90, 0x90, // A
  0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
  0xF0, 0x80, 0x80, 0x80, 0xF0, // C
  0xE0, 0x90, 0x90, 0x90, 0xE0, // D
  0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
  0xF0, 0x80, 0xF0, 0x80, 0x80  // F
];

const KEY_LAYOUT: [Key; 16] =
[
    Key::Num1, Key::Num2, Key::Num3, Key::Num4,
    Key::Q, Key::W, Key::E, Key::R,
    Key::A, Key::S, Key::D, Key::F,
    Key::Z, Key::X, Key::C, Key::V
];

struct Chip8 {
    opcode: u16,
    memory: [u8; 4096],
    v: [u8; 16], // register
    i: u16, // index
    pc: u16, // program counter
    gfx: [bool; DISPLAY_SIZE],
    stack: Vec<u16>,
    key: [bool; 16],
    draw_flag: bool,
    delay_timer: u8,
    sound_timer: u8
}

const INSTRUCTION_DELAY: time::Duration = time::Duration::from_micros(500);

fn main() -> Result<(), &'static str> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        return Err("You must pass a chip8 programm to the emulator.");
    }

    let mut chip8 = Chip8::new();
    chip8.init();
    chip8.load_game(&args[1]);
    let mut window = Window::new("Chip 8 emulator", DISPLAY_WIDTH * FACTOR, DISPLAY_HEIGHT * FACTOR);

    // init audio
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let sink = Sink::try_new(&stream_handle).unwrap();
    sink.append(SineWave::new(440.0));

    let mut counter = 0;
    loop {
        let start = time::Instant::now();
        if counter % 32 == 0 { // 32 = 16ms
            if !window.next_frame() { return Ok(()); } // update display at 60hz
            if chip8.delay_timer > 0 { chip8.delay_timer -= 1; };
            if chip8.sound_timer > 0 {
                sink.play();
                chip8.sound_timer -= 1;
            } else {
                sink.pause();
            }
        }

        if counter % 4 == 0 { // 2 ms
            update_keys(&mut chip8, &window);
            chip8.fetch_opcode();
            chip8.emulate_cycle();
            if chip8.draw_flag {
                update_screen(&chip8, &mut window);
                chip8.draw_flag = false;
            }
        }

        if counter == 1000 {
            counter = 0;
        } else {
            counter += 1;
        }
        let took = time::Instant::now().duration_since(start);
        sleep(INSTRUCTION_DELAY.saturating_sub(took));
    }
}

const FACTOR: u16 = 10;

fn update_keys(chip: &mut Chip8, window: &Window) {
    for (i, key) in KEY_LAYOUT.iter().enumerate() {
        if window.is_key_down(*key) {
            chip.key[i] = true;
        } else {
            chip.key[i] = false;
        }
    }
}

fn update_screen(chip: &Chip8, window: &mut Window) {
    window.clear();
    for (i, pixel) in chip.gfx.iter().enumerate() {
        if *pixel {
            let i = i as u16;
            let height = i / DISPLAY_WIDTH * FACTOR;
            let width = i % DISPLAY_WIDTH * FACTOR;
            let rect = Rect::new(width as i32, height as i32, FACTOR as u32, FACTOR as u32);
            window.fill_rect(rect);
        }
    }
}

impl Chip8 {
    fn new() -> Chip8 {
        Chip8 {
            opcode: 0,
            memory: [0; 4096],
            v: [0; 16],
            i: 0,
            pc: 0x200,
            gfx: [false; DISPLAY_SIZE],
            stack: Vec::new(),
            key: [false; 16],
            draw_flag: false,
            delay_timer: 0,
            sound_timer: 0
        }
    }

    fn init(&mut self) {
        for (i, byte) in FONTSET.iter().enumerate() {
            self.memory[i + FONTSET_OFFSET as usize] = *byte;
        }
    }

    fn load_game(&mut self, file: &str) {
        let path = Path::new(file);
        let mut file = File::open(&path).expect("File not found!");
        file.read(&mut self.memory[0x200..]).expect("Failed to read file!");
    }

    fn fetch_opcode(&mut self) {
        let pc = self.pc as usize;
        self.opcode = (self.memory[pc] as u16) << 8 | self.memory[pc + 1] as u16;
        self.pc += 2;
    }

    fn v_opcode(&self, opcode: u16, pattern: u16, shift: u8) -> u8 {
        self.v[extract_usize(opcode, pattern, shift)]
    }

    fn emulate_cycle(&mut self) {
        let opcode = self.opcode;
        //println!("debug cycle. OpCode: {:x}, pc: {:x}", opcode, self.pc);
        match extract(opcode, 0xF000, 3) {
            0 => match extract(opcode, 0x00FF, 0) {
                0xE0 =>  self.gfx = [false; DISPLAY_SIZE],
                0xEE => self.pc = self.stack.pop().expect("Failed to pop from stack!"),
                _ => invalid_opcode(opcode)
            },
            1 => self.pc = opcode & 0x0FFF,
            2 => {
                self.stack.push(self.pc);
                self.pc = extract(opcode, 0x0FFF, 0);
            }
            3 => if self.v_opcode(opcode, 0x0F00, 2) == extract(opcode, 0x00FF, 0) as u8 { self.pc += 2; }
            4 => if self.v_opcode(opcode, 0x0F00, 2) != extract(opcode, 0x00FF, 0) as u8 { self.pc += 2; },
            5 => if self.v_opcode(opcode, 0x0F00, 2) == self.v_opcode(opcode, 0x00F0, 1) { self.pc += 2; },
            6 => self.v[extract_usize(opcode, 0x0F00, 2)] = extract(opcode, 0x00FF, 0) as u8,
            7 => self.v[extract_usize(opcode, 0x0F00, 2)] = (self.v_opcode(opcode, 0x0F00, 2) as u16 + extract(opcode, 0x00FF, 0)) as u8,
            8 => match extract(opcode, 0x000F, 0) {
                0 => self.v[extract_usize(opcode, 0x0F00, 2)] = self.v_opcode(opcode, 0x00F0, 1),
                1 => self.v[extract_usize(opcode, 0x0F00, 2)] = self.v_opcode(opcode, 0x0F00, 2) | self.v_opcode(opcode, 0x00F0, 1),
                2 => self.v[extract_usize(opcode, 0x0F00, 2)] = self.v_opcode(opcode, 0x0F00, 2) & self.v_opcode(opcode, 0x00F0, 1),
                3 => self.v[extract_usize(opcode, 0x0F00, 2)] = self.v_opcode(opcode, 0x0F00, 2) ^ self.v_opcode(opcode, 0x00F0, 1),
                4 => {
                    let result = self.v_opcode(opcode, 0x0F00, 2) as u16 + self.v_opcode(opcode, 0x00F0, 1) as u16;
                    self.v[0xF] = (result > 255) as u8;
                    self.v[extract_usize(opcode, 0x0F00, 2)] = result as u8;
                }
                5 => {
                    let (diff, carry) = subtract(self.v_opcode(opcode, 0x0F00, 2), self.v_opcode(opcode, 0x00F0, 1));
                    self.v[0xF] = carry;
                    self.v[extract_usize(opcode, 0x0F00, 2)] = diff;
                }
                6 => {
                    self.v[0xF] = self.v_opcode(opcode, 0x0F00, 2) & 1;
                    self.v[extract_usize(opcode, 0x0F00, 2)] >>= 1;
                    }
                0xE => {
                    self.v[0xF] = self.v_opcode(opcode, 0x0F00, 2) & 0x80;
                    self.v[extract_usize(opcode, 0x0F00, 2)] <<= 1;
                }
                7 => {
                    let (diff, carry) = subtract(self.v_opcode(opcode, 0x00F0, 1), self.v_opcode(opcode, 0x0F00, 2));
                    self.v[0xF] = carry;
                    self.v[extract_usize(opcode, 0x0F00, 2)] = diff;
                }
                _ => invalid_opcode(opcode)
            }
            9 => if self.v_opcode(opcode, 0x0F00, 2) != self.v_opcode(opcode, 0x00F0, 1) { self.pc += 2; }
            0xA => self.i = extract(opcode, 0x0FFF, 0),
            0xB => self.pc = extract(opcode, 0x0FFF, 0) + self.v[0] as u16,
            0xC => self.v[extract_usize(opcode, 0x0F00, 2)] = random::<u8>() & extract(opcode, 0x00FF, 0) as u8,
            0xE => match extract(opcode, 0x00FF, 0) {
                0x9E => if self.key[self.v_opcode(opcode, 0x0F00, 2) as usize] { self.pc += 2; },
                0xA1 => if !self.key[self.v_opcode(opcode, 0x0F00, 2) as usize] { self.pc += 2; },
                _ => invalid_opcode(opcode)
            }
            0xD => {
                let x = self.v[extract_usize(opcode, 0x0F00, 2)] % DISPLAY_WIDTH as u8;
                let y = self.v[extract_usize(opcode, 0x00F0, 1)];
                let n = extract(opcode, 0x000F, 0);
                self.v[0xF] = 0;
                self.draw_flag = true;

                for i in 0..n {
                    let y = y + i as u8;
                    let byte = self.memory[(self.i + i) as usize];

                    for bit in BitIter::from(byte.reverse_bits()) { // we need to iterate from right to left, so the lsb must be the msb
                        let x = x + bit as u8;

                        if x >= DISPLAY_WIDTH as u8 || y >= DISPLAY_HEIGHT as u8 { break; };

                        let sub_y = if y == 0 { 0 } else { y - 1};
                        let screen_pixel = ((sub_y as u16) * DISPLAY_WIDTH + x as u16) as usize;

                        if self.gfx[screen_pixel] {
                            self.v[0xF] = 1;
                        }
                        self.gfx[screen_pixel] = !self.gfx[screen_pixel];
                    }
                }
            }
            0xF => match extract(opcode, 0x00FF, 0) {
                7 => self.v[extract_usize(opcode, 0x0F00, 2)] = self.delay_timer,
                0x15 => self.delay_timer = self.v_opcode(opcode, 0x0F00, 2),
                0x18 => self.sound_timer = self.v_opcode(opcode, 0x0F00, 2),
                0x1E => {
                    self.i += self.v_opcode(opcode, 0x0F00, 2) as u16;
                    self.v[0xF] = (self.i > 1000) as u8;
                }
                0x0A => {
                    for (i, key) in self.key.iter().enumerate() {
                        if *key {
                            self.v[extract_usize(opcode, 0x0F00, 2)] = i as u8;
                            return;
                        }
                    }
                    self.pc -= 2;
                }
                0x29 => self.i = (self.v_opcode(opcode, 0x0F00, 2) & 0x0F) as u16 * 5 + FONTSET_OFFSET as u16,
                0x33 => {
                    let num = self.v_opcode(opcode, 0x0F00, 2).to_string();
                    for (i, num) in num.bytes().enumerate() {
                        self.memory[self.i as usize + i] = num - 48;
                    }
                }
                0x55 => {
                    for i in 0..=extract(opcode, 0x0F00, 2) {
                        self.memory[(self.i + i) as usize] = self.v[i as usize];
                    }
                }
                0x65 => {
                    for i in 0..=extract(opcode, 0xF00, 2) {
                        self.v[i as usize] = self.memory[(self.i + i) as usize];
                    }
                }
                _ => invalid_opcode(opcode)
            }
            _ => invalid_opcode(opcode)
            }
    }
}

fn subtract(minuend: u8, subtrahend: u8) -> (u8, u8) {
    let difference = minuend as i16 - subtrahend as i16;
    let carry = minuend > subtrahend;
    (difference as u8, carry as u8)
}

fn extract_usize(opcode: u16, pattern: u16, shift: u8) -> usize {
    extract(opcode, pattern, shift) as usize
}

fn extract(opcode: u16, pattern: u16, shift: u8) -> u16 {
    (opcode & pattern) >> shift * 4
}

fn invalid_opcode(opcode: u16) {
    panic!("OpCode not found: {:x}", opcode);
}
