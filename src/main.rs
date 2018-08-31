extern crate dbus;
extern crate regex;

// use std::collections::HashMap;
use std::iter::FromIterator;
use std::error::*;
use std::fmt;

use dbus::{Connection, BusType, Message};
use dbus::arg::{Array, Dict, Variant, RefArg};

const XFCONF_BUS  : &str = "org.xfce.Xfconf";
const XFCONF_PATH : &str = "/org/xfce/Xfconf";
const XFCONF_OBJ  : &str = "org.xfce.Xfconf";
const XFCONF_DESKTOP_CHANNEL : &str = "xfce4-desktop";

struct XFCEDesktop {
    conn: Connection,
    // monitors: Vec<String>,
}

#[derive(Debug)]
enum XFConfError {
    CallError(String),
    DBusError(dbus::Error),
}

impl From<dbus::Error> for XFConfError {
    fn from(e: dbus::Error) -> Self {
        XFConfError::DBusError(e)
    }
}

impl From<String> for XFConfError {
    fn from(s: String) -> Self {
        XFConfError::CallError(s)
    }
}

impl Error for XFConfError {
    fn description(&self) -> &str {
        match self {
            &XFConfError::CallError(ref s) => s.as_str(),
            &XFConfError::DBusError(ref dbe) => dbe.message().unwrap_or("<NoMessage>"),
        }
    }

    fn cause(&self) -> Option<&Error> {
        None
    }
}

impl fmt::Display for XFConfError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "XFConf Error : {}", self.description())
    }
}

impl XFCEDesktop {
    fn new() -> Result<XFCEDesktop, dbus::Error> {
        let xfcedesktop = XFCEDesktop {
            conn: Connection::get_private(BusType::Session)?,
        };
        Ok(xfcedesktop)
    }

    #[allow(dead_code)]
    fn call_func0(&self, method: &str) -> Result<Message, XFConfError> {
        let m = Message::new_method_call(XFCONF_BUS, XFCONF_PATH, XFCONF_OBJ, method)?;
        Ok(self.conn.send_with_reply_and_block(m, 2000)?)
    }

    fn call_func2(&self, method: &str, p1: &str, p2: &str) -> Result<Message, XFConfError> {
        let m = Message::new_method_call(XFCONF_BUS, XFCONF_PATH, XFCONF_OBJ, method)?.append2(p1, p2);
        Ok(self.conn.send_with_reply_and_block(m, 2000)?)
    }

    #[allow(dead_code)]
    fn get_channels(&self) -> Result<Option<Vec<String>>,XFConfError> {
        let m = self.call_func0("ListChannels")?;
        let arr: Array<&str,_> = m.get1().unwrap();
        Ok(Some(Vec::from_iter(arr.map(|x| x.to_owned()))))
    }

    fn prop_test(&self) -> Result<(),XFConfError> {
        let m = self.call_func2("GetAllProperties", XFCONF_DESKTOP_CHANNEL, "/backdrop/screen0")?;
        let z: Dict<&str, Variant<Box<RefArg>>, _> = m.get1().unwrap();
        let mut keys = z.map(|(x,_)| x).collect::<Vec<&str>>();
        keys.sort();

        // let actual_monitor_re = Regex::new("/backdrop/screen0/monitorHDMI-0/workspace0/color-style").map_err(?;
        for k in keys {
            println!("{}", k);
        }
        Ok(())
    }
}

fn main() {
    let xd = XFCEDesktop::new().unwrap();
    match xd.prop_test() {
        Ok(_) => (),
        Err(e) => println!("Boinked.. {}", e)
    }
}
