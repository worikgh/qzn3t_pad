//! Use a Novation LPX Pad as a musical instrument
//! Control the colours on the display
//! Translate the MIDI signals from the raw PAD number from the LPX into
//! noteon/noteoff signals
//! On start up connect directly to the LPX (it must exist ad be
//! available) then set up a virtual connection for the synthesiser
//! and connect to it later
//! ["Programmer's Manual" ](https://fael-downloads-prod.focusrite.com/customer/prod/s3fs-public/downloads/Launchpad%20X%20-%20Programmers%20Reference%20Manual.pdf)
extern crate midir;
extern crate serde;
// mod lpx_ctl_error;
mod section;

use crate::midir::os::unix::VirtualOutput;
use crate::section::Section;
use crate::section::default_sections;
use clap::{Arg, Command};
use midir::MidiInputPort;
use midir::MidiOutputPort;
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::process::exit;
use std::result::Result;
use std::sync::mpsc::{self, Receiver, Sender};

/// Initialise a vector of `Section` from a file.
fn load_sections(filename: &str) -> Option<Vec<Section>> {
    let mut file = match File::open(filename) {
        Ok(f) => f,
        Err(err) => panic!("{err}"),
    };
    let mut content = String::new();
    match file.read_to_string(&mut content) {
        Ok(_) => (),
        Err(err) => panic!("{err}"),
    };

    // Create the sections from the file
    let mut sections: Vec<Section> = Section::parse_json(&content).expect("Failed parsing JSON");
    // If there is a default section with no pads put all unincluded pads in it
    if let Some(index) = sections.iter().position(|x| x.pads.is_empty()) {
        // Collect all pads mentioned so far
        let mut pads_here: Vec<u8> = sections.iter().flat_map(|x| x.pads.clone()).collect();
        if pads_here.len() < 64 {
            // Need the default
            pads_here.sort();
            // Check each row for missing pads and add them to default
            for r in 1..=8 {
                let pads: Vec<&u8> = pads_here.iter().filter(|x| *x / 10 == r).collect();
                for c in 1..=8 {
                    let pad = r * 10 + c;
                    if !pads.iter().any(|x| x == &&pad) {
                        sections[index].pads.push(pad);
                    }
                }
            }
        }
    }
    Some(sections)
}

// Get a MIDI port that has a name containing `keyword`
fn get_midi_port<T: midir::MidiIO>(midi_io: &T, keyword: &str) -> Option<T::Port> {
    for port in midi_io.ports() {
        let name = match midi_io.port_name(&port) {
            Ok(name) => name,
            Err(_) => continue,
        };
        // eprintln!("DBG qzn3t_pad: get_midi_port(midi_io, {keyword}) name: {name}");

        if name.contains(keyword) {
            eprintln!(
                "DBG qzn3t_pad: get_midi_port(midi_io, {keyword}) from keyword: {keyword} name: {name}"
            );
            return Some(port);
        }
    }

    None
}

/// Create an output MIDI port to the LPX.
/// It uses the passed parameter `name` to create a port: LpxCtl:<name>
fn get_midi_out(name: &str, p_name: &str) -> Result<MidiOutputConnection, Box<dyn Error>> {
    let midi_output = MidiOutput::new("LpxCtl")?;
    let port = get_midi_port(&midi_output, p_name)
        .ok_or(format!["Failed to get MIDI port {p_name} -> PAD"])?;
    Ok(midi_output.connect(&port, name)?)
}

/// Create a MIDI input port, connected from the LPX MIDI port.
/// `name` is the port name for the created port
/// `f` is the function that takes a channel and sends the MIDI that
/// it wants to handle down that channel
/// `tx` is the channel
fn get_midi_in(
    name: &str,
    p_name: &str,
    f: impl FnMut(u64, &[u8], &mut Sender<[u8; 3]>) + Send + 'static,
    tx: Sender<[u8; 3]>,
) -> Result<MidiInputConnection<Sender<[u8; 3]>>, Box<dyn Error>> {
    let midi_input = MidiInput::new(p_name)?;
    let port = match get_midi_port(&midi_input, p_name) {
        Some(p) => p,
        None => panic!("Failed to find port: {p_name} in get_midi_in"),
    };
    //.ok_or(Err("Failed guess port".into())?);
    let result = midi_input.connect(&port, name, f, tx)?;
    Ok(result)
}

/// Get all MIDI Ports.
fn get_all_midi_input_ports() -> Result<Vec<String>, Box<dyn Error>> {
    let input = MidiInput::new("foo")?;
    let ports: Vec<MidiInputPort> = input.ports();
    let mut result: Vec<String> = vec![];
    for p in ports {
        result.push(input.port_name(&p)?);
    }
    Ok(result)
}
fn get_all_midi_output_ports() -> Result<Vec<String>, Box<dyn Error>> {
    let output = MidiOutput::new("foo")?;
    let ports: Vec<MidiOutputPort> = output.ports();
    let mut result: Vec<String> = vec![];
    for p in ports {
        result.push(output.port_name(&p)?);
    }
    Ok(result)
}

fn main() -> Result<(), Box<dyn Error>> {
    let matches = Command::new("qzn3t_pad")
        .arg(
            Arg::new("list")
                .short('l')
                .long("list")
                .help("List available configurations")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .help("Path to configuration file")
                .value_name("FILE")
                .required(false),
        )
        .arg(
            Arg::new("pad_midi_out")
                .short('o')
                .long("pad_midi_out")
                .help("Send MIDI to pad on this port")
                .value_name("MIDI_OUT")
                .default_value("Launchpad X LPX MIDI In")
                .required(false),
        )
        .arg(
            Arg::new("pad_midi_in")
                .short('i')
                .long("pad_midi_in")
                .help("Get MIDI from pad on this port")
                .value_name("MIDI_IN")
                .default_value("Launchpad X LPX MIDI In")
                .required(false),
        )
        .get_matches();

    if *matches.get_one::<bool>("list").unwrap() {
        println!("Input ports:");
        let ports = get_all_midi_input_ports()?;
        for port_name in ports {
            println!("\t{port_name}");
        }
        println!("Output ports:");
        let ports = get_all_midi_output_ports()?;
        for port_name in ports {
            println!("\t{port_name}");
        }
        exit(0);
    }

    let midi_input = matches.get_one::<String>("pad_midi_in").unwrap();
    let midi_output = matches.get_one::<String>("pad_midi_out").unwrap();

    println!("MIDI  input: {}", midi_input);
    println!("MIDI output: {}", midi_output);

    // Initialise the collection of `Section` from the file. (See `section.rs`)
    let sections: Vec<Section> = if let Some(cfg_file_name) = matches.get_one::<String>("config") {
        load_sections(cfg_file_name).expect("Failed to load sections")
    } else {
        default_sections()
    };

    // The channel to send MIDI messages, received from the LPX in the
    // MidiInputConnection, here to the main thread
    let (tx, rx): (Sender<[u8; 3]>, Receiver<[u8; 3]>) = mpsc::channel::<[u8; 3]>();

    // Connect to the LPX to receive pad press events.  `f` is the
    // function that handles input MIDI and sends them back to themain
    // thread
    let f = move |_stamp, message: &[u8], tx: &mut Sender<[u8; 3]>| {
        // let message = MidiMessage::from_bytes(message.to_vec());
        if message.len() == 3 {
            let m3: [u8; 3] = message.try_into().unwrap();
            tx.send(m3).unwrap();
        }
    };
    // The port stays open as long as `_in` is in scope
    let _in = get_midi_in("read_input", midi_output.as_str(), f, tx.clone())?;

    // Create an output port to the LPX for sending it colour.
    let mut colour_port: MidiOutputConnection = get_midi_out("colour_port", midi_input.as_str())?;

    // Selecting Layouts (page 7 programmers manual).  127 => "Programmer Mode"
    let msg: [u8; 9] = [240, 0, 32, 41, 2, 12, 0, 127, 247];
    match colour_port.send(&msg) {
        Ok(()) => (),
        Err(err) => eprintln!("Error qzn3t_pad: {err}: Failed to send msg to LPX: {msg:?}"),
    };

    let make_colour = |section: &Section, colour: [u8; 3]| -> Vec<u8> {
        // Buid the MIDI command that sets the colours of all the pads
        // in a section (they are all the same colour - part of what
        // defines a section).  One long MIDI sysex message that sets
        // many pads in one command

        // "LED lighting SysEx message" programmer's mabual page 15
        let mut colour_message: Vec<u8> = vec![240, 0, 32, 41, 2, 12, 3];
        let pads: Vec<u8> = section.pads().to_vec();
        for pad in pads.iter() {
            colour_message.push(3); // RGB colour
            colour_message.push(*pad); // Pad index
            colour_message.extend(colour.to_vec()); // RGB tripple
        }
        colour_message.push(247); // End message
        colour_message
    };

    // Initialise the colours
    for section in sections.iter() {
        let colour = make_colour(section, section.main_colour);
        match colour_port.send(&colour) {
            Ok(()) => (),
            Err(err) => eprintln!("Error qzn3t_pad: {err}: Cannot send colour: {colour:?}"),
        };
    }

    // Establish the output that sends MIDI to whatever software will
    // interpret the MIDI to create sound and MIDI controls to
    // whatever interprets them.  An external programme will have to
    // conmplete these setups as this programme does not know what
    // they will be
    let midi_out: MidiOutput = MidiOutput::new("LpxCtlNote")?;
    let port_name = "port";
    let mut midi_note_out_port: MidiOutputConnection = midi_out.create_virtual(port_name)?;

    let midi_out: MidiOutput = MidiOutput::new("LpxCtlCtl")?;
    let port_name = "port";
    let mut midi_ctl_out_port: MidiOutputConnection = midi_out.create_virtual(port_name)?;
    eprintln!("DBG qzn3t_pad: Virtual MIDI Output port 'LpxCtlNote:{port_name}' is open");
    eprintln!("DBG qzn3t_pad: Virtual MIDI Output port 'LpxCtlCtl:{port_name}' is open");

    // Main loop.
    loop {
        let message: [u8; 3] = match rx.recv() {
            Ok(m) => m,
            Err(err) => panic!("{}", err),
        };
        eprint!(
            "MIDI: {:2x} {:2x} {:2x}.  ",
            message[0], message[1], message[2]
        );
        if message[0] == 144 {
            // All MIDI notes from LPX start with 144, for initial
            // noteon and noteoff

            // Find the section the pad is in
            let pad: u8 = message[1];

            if let Some(section) = sections.iter().find(|x| x.pad_in(pad)) {
                // got the section for a pad

                // Send out the note
                let velocity = message[2];
                let message: [u8; 3] = [message[0], section.midi_note, velocity];
                eprintln!(
                    "DBG qzn3t_pad: SEND NoteOn: Type: {:2x} Note: {:2x} Velocity: {:2x}",
                    message[0], message[1], message[2]
                );
                midi_note_out_port.send(&message)?;

                if velocity > 0 {
                    // Note on
                    // Set colour of section to "active_colour"
                    let active_colour = make_colour(section, section.active_colour);
                    colour_port.send(&active_colour).unwrap();
                } else {
                    // Note off
                    // Restore the colour
                    let main_colour = make_colour(section, section.main_colour);
                    colour_port.send(&main_colour).unwrap();
                }
                continue;
            }
        } else if message[0] == 176 {
            // A control signal
            eprintln!(
                "DBG qzn3t_pad: SEND Ctl: {:2x} {:2x} {:2x}",
                message[0], message[1], message[2]
            );
            midi_ctl_out_port.send(&message).unwrap();
        }
    }
    // Ok(())
}
//
